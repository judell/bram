"""Where a Bram project's state folder is: `.bram/`.

The same rule as the host's `bram_state_dir` (src-tauri/src/lib.rs) for a
writable project: `<root>/.bram`. Since 0.8.7 (issue-469-retire-resources) a
project's state is moved there at startup, or the start is held, so nothing
reads the pre-0.8.4 `resources/` any more. The one exception is the host's:
the home directory, where `~/.bram` is the machine folder (it holds the
bram-guard link), never a project's state, so a home root answers
`resources` there as it does in the host, and nothing is ever read from or
written to `~/.bram` as state.

Scripts import it as a sibling (`import bram_state`): the script's own
directory is on sys.path even under `python3 -I`.
"""

from __future__ import annotations

import os
from pathlib import Path

STATE_DIR_NEW = ".bram"
# The host's answer for a home root only (see above).
_HOME_STATE_DIR = "resources"


def _is_home(root: Path) -> bool:
    home = os.path.expanduser("~")
    if not home or home == "~":
        return False
    try:
        return Path(root).resolve() == Path(home).resolve()
    except OSError:
        return False


def is_bram_state_dir(root: Path | str) -> bool:
    """Is `<root>/.bram` Bram's (#469)? Every folder Bram makes carries
    `.migrated-from` or a `.gitignore` of exactly `*`; a `.bram/` with
    neither belongs to something else (the host then holds the start)."""
    dot = Path(root) / STATE_DIR_NEW
    if (dot / ".migrated-from").exists():
        return True
    try:
        return (dot / ".gitignore").read_text(encoding="utf-8").strip() == "*"
    except OSError:
        return False


def state_dir_name(root: Path | str) -> str:
    return _HOME_STATE_DIR if _is_home(Path(root)) else STATE_DIR_NEW


def state_dir(root: Path | str) -> Path:
    return Path(root) / state_dir_name(root)


def port_file(root: Path | str) -> Path:
    return state_dir(root) / ".bram-port"


def trace_dir(root: Path | str) -> Path:
    return state_dir(root) / "bram-traces"


def trace_log(root: Path | str) -> Path:
    return trace_dir(root) / "bram-trace.log"
