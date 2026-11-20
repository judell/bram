#!/usr/bin/env python3
"""Static server for the xmlui navigation repro (judell/bram#371).

Serves this directory with SPA fallback and answers POST /query with one row,
which is all the repro needs from a /query backend. Point a browser at
http://127.0.0.1:8791/one and follow README.md.
"""
import json
import os
import sys
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer

ROOT = os.path.dirname(os.path.abspath(__file__))
PORT = int(os.environ.get("REPRO_PORT", "8791"))

# Serve the REAL vendored bundle rather than a copy: the whole point is to
# verify the artifact we actually ship, and a copy here would go stale.
VENDOR = os.path.normpath(
    os.path.join(ROOT, "..", "..", "app", "vendor", "xmlui-standalone.umd.js")
)


class Handler(SimpleHTTPRequestHandler):
    def translate_path(self, path):
        rel = path.split("?", 1)[0].split("#", 1)[0].lstrip("/")
        if rel == "xmlui-standalone.umd.js":
            return VENDOR
        full = os.path.join(ROOT, rel)
        if rel and os.path.isfile(full):
            return full
        return os.path.join(ROOT, "index.html")  # SPA fallback

    def do_POST(self):
        if self.path.split("?")[0] != "/query":
            self.send_error(404)
            return
        length = int(self.headers.get("Content-Length") or 0)
        self.rfile.read(length)
        body = json.dumps([{"n": 1}]).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, fmt, *args):
        sys.stderr.write("%s %s\n" % (self.command, self.path))


if __name__ == "__main__":
    print("serving %s on http://127.0.0.1:%d/one" % (ROOT, PORT), file=sys.stderr)
    ThreadingHTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
