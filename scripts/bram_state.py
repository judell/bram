"""Where a Bram project's state folder is: `.bram/` or `resources/`.

The same rule as the host's `bram_state_dir` (src-tauri/src/lib.rs):
`<root>/.bram` when that directory exists, else `<root>/resources`. The
host also ignores a `.bram/` in the home directory, so this does too.
0.8.3 accepts both names; 0.8.4 moves the folder to `.bram/` (#469).

Scripts import it as a sibling (`import bram_state`): the script's own
directory is on sys.path even under `python3 -I`.
"""

from __future__ import annotations

import os
from pathlib import Path

STATE_DIR_NEW = ".bram"
STATE_DIR_LEGACY = "resources"


def _is_home(root: Path) -> bool:
    home = os.path.expanduser("~")
    if not home or home == "~":
        return False
    try:
        return Path(root).resolve() == Path(home).resolve()
    except OSError:
        return False


def state_dir_name(root: Path | str) -> str:
    root = Path(root)
    if (root / STATE_DIR_NEW).is_dir() and not _is_home(root):
        return STATE_DIR_NEW
    return STATE_DIR_LEGACY


def state_dir(root: Path | str) -> Path:
    return Path(root) / state_dir_name(root)


def port_file(root: Path | str) -> Path:
    return state_dir(root) / ".bram-port"


def trace_dir(root: Path | str) -> Path:
    return state_dir(root) / "bram-traces"


def trace_log(root: Path | str) -> Path:
    return trace_dir(root) / "bram-trace.log"
