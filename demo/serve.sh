#!/usr/bin/env python3
"""Serve the QQL Playground with aggressive no-cache headers."""
import http.server
import os
import sys

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8080

class NoCacheHandler(http.server.SimpleHTTPRequestHandler):
    def end_headers(self):
        self.send_header("Cache-Control", "no-store, no-cache, must-revalidate, max-age=0")
        self.send_header("Pragma", "no-cache")
        self.send_header("Expires", "0")
        super().end_headers()

os.chdir(os.path.dirname(os.path.abspath(__file__)))
print(f"QQL Playground → http://localhost:{PORT}")
print("No-cache headers enabled. Ctrl+C to stop.")
http.server.HTTPServer(("0.0.0.0", PORT), NoCacheHandler).serve_forever()
