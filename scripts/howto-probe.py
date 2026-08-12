#!/usr/bin/env python3
"""Probe xmlui_search_howto against a pinned docs corpus.

Usage: howto-probe.py <version|latest> <query>

Spawns `xmlui mcp [--xmlui-version <v>]` as a stdio MCP server, issues one
xmlui_search_howto call, prints the headline (query/confidence line) and the
top hits. This is the before/after driver for the gap-close regression test:
the same query run at a pre-doc pin and a post-doc pin should flip from
low/medium to high with the target doc as the #1 hit.
"""
import json
import subprocess
import sys
import threading

def main():
    version, query = sys.argv[1], sys.argv[2]
    cmd = ["xmlui", "mcp"]
    if version != "latest":
        cmd += ["--xmlui-version", version]
    proc = subprocess.Popen(
        cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL, text=True, bufsize=1,
    )
    # Hard timeout so a wedged server can't hang the harness.
    timer = threading.Timer(60, proc.kill)
    timer.start()

    def send(obj):
        proc.stdin.write(json.dumps(obj) + "\n")
        proc.stdin.flush()

    def read_until(rid):
        while True:
            line = proc.stdout.readline()
            if not line:
                raise SystemExit(f"server exited before responding (id={rid})")
            try:
                msg = json.loads(line)
            except json.JSONDecodeError:
                continue  # stray non-JSON stdout line
            if msg.get("id") == rid:
                return msg

    send({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2024-11-05", "capabilities": {},
        "clientInfo": {"name": "gap-close-probe", "version": "0"}}})
    read_until(1)
    send({"jsonrpc": "2.0", "method": "notifications/initialized"})
    send({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {
        "name": "xmlui_search_howto", "arguments": {"query": query}}})
    resp = read_until(2)
    timer.cancel()
    proc.terminate()

    text = "".join(
        c.get("text", "") for c in resp.get("result", {}).get("content", [])
    )
    print(f"### corpus pin: {version}")
    for line in text.splitlines():
        if line.startswith("Query:") or line.startswith("## "):
            print(line)

if __name__ == "__main__":
    main()
