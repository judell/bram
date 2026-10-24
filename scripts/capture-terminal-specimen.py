#!/usr/bin/env python3
"""Capture what an agent CLI draws, for terminal-attention specimens.

Runs a command under a pseudo-terminal of a fixed size, sends nothing (or the
keys you give), and writes the raw bytes and an ANSI-stripped copy. The
stripped text is what terminal_attention_prompt_shape's markers are written
against, and what its tests pin. See docs/developing-bram.md, "Capturing
terminal specimens".

    python3 -I scripts/capture-terminal-specimen.py OUT_PREFIX SECONDS -- CMD...
    python3 -I scripts/capture-terminal-specimen.py --keys $'\\r' --after 4 \\
        /tmp/spec/codex 12 -- codex

Writes OUT_PREFIX.raw and OUT_PREFIX.txt. The caller owns the environment:
point the CLI at a throwaway config directory and unset API keys, so no real
account or setting is touched.
"""
import argparse
import fcntl
import os
import pty
import re
import select
import signal
import struct
import sys
import termios
import time


def strip_ansi(text):
    text = re.sub(r"\x1b\][^\x07\x1b]*(\x07|\x1b\\)", "", text)  # OSC
    text = re.sub(r"\x1b\[[0-9;?<>=]*[ -/]*[@-~]", "", text)  # CSI
    text = re.sub(r"\x1b[()][0-9A-Za-z]", "", text)  # charset
    text = re.sub(r"\x1b[=>78]", "", text)
    return text


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("out_prefix")
    ap.add_argument("seconds", type=float)
    ap.add_argument("cmd", nargs=argparse.REMAINDER)
    ap.add_argument("--cols", type=int, default=120)
    ap.add_argument("--rows", type=int, default=40)
    ap.add_argument("--keys", default="", help="bytes to send once, e.g. $'\\r'")
    ap.add_argument("--after", type=float, default=0.0, help="seconds before --keys")
    args = ap.parse_args()
    cmd = args.cmd[1:] if args.cmd[:1] == ["--"] else args.cmd
    if not cmd:
        ap.error("no command given")

    pid, fd = pty.fork()
    if pid == 0:
        os.environ["COLUMNS"], os.environ["LINES"] = str(args.cols), str(args.rows)
        os.execvp(cmd[0], cmd)
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", args.rows, args.cols, 0, 0))

    buf = b""
    start = time.time()
    sent = not args.keys
    while time.time() - start < args.seconds:
        if not sent and time.time() - start >= args.after:
            os.write(fd, args.keys.encode())
            sent = True
        ready, _, _ = select.select([fd], [], [], 0.2)
        if ready:
            try:
                chunk = os.read(fd, 65536)
            except OSError:
                break
            if not chunk:
                break
            buf += chunk
    for sig in (signal.SIGTERM, signal.SIGKILL):
        try:
            os.kill(pid, sig)
        except ProcessLookupError:
            break
        time.sleep(0.3)

    with open(args.out_prefix + ".raw", "wb") as f:
        f.write(buf)
    with open(args.out_prefix + ".txt", "w") as f:
        f.write(strip_ansi(buf.decode("utf-8", "replace")))
    print(f"{len(buf)} bytes -> {args.out_prefix}.txt", file=sys.stderr)


if __name__ == "__main__":
    main()
