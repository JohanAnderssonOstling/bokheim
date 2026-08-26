#!/usr/bin/env python3

from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


class CrossOriginIsolatedHandler(SimpleHTTPRequestHandler):
    def end_headers(self) -> None:
        self.send_header("Cross-Origin-Opener-Policy", "same-origin")
        self.send_header("Cross-Origin-Embedder-Policy", "require-corp")
        self.send_header("Cache-Control", "no-store")
        super().end_headers()


dist = Path(__file__).resolve().parent.parent / "dist"
handler = partial(CrossOriginIsolatedHandler, directory=dist)
ThreadingHTTPServer(("127.0.0.1", 4174), handler).serve_forever()
