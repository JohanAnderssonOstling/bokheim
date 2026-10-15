#!/usr/bin/env python3
"""Check browser controls while a page's JavaScript is hung.

Run after building the helper, on an isolated X11 display:
    xvfb-run -a python3 apps/opds-webview/tests/responsiveness.py --display-env
Requires Python GTK 3 bindings and xdotool.
"""
import argparse
import json
import os
import select
import subprocess
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlparse

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--display', help='An isolated X11 test display')
parser.add_argument('--display-env', action='store_true', help='Use DISPLAY supplied by xvfb-run')
parser.add_argument('--helper', default='target/release/bokheim-opds-webview')
args = parser.parse_args()
if not args.display and not args.display_env:
    parser.error('specify an isolated --display or use xvfb-run with --display-env')
if args.display:
    os.environ['DISPLAY'] = args.display
os.environ['GDK_BACKEND'] = 'x11'
os.environ.pop('WAYLAND_DISPLAY', None)

import gi
gi.require_version('Gtk', '3.0')
from gi.repository import Gtk, Gdk
Gtk.init([])

loaded = threading.Event()
hung = threading.Event()
recovered = threading.Event()


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        path = urlparse(self.path).path
        if path == '/loaded':
            loaded.set()
            body = b'ok'
        elif path == '/hung':
            hung.set()
            body = b'ok'
        elif path == '/recovered':
            recovered.set()
            body = b'ok'
        elif path == '/hang':
            body = b'''<html><body><button id="hang" style="position:absolute;left:10px;top:10px">Hang page</button>
            <script>addEventListener('pageshow', () => fetch('/loaded')); hang.onclick = async () => {await fetch('/hung'); while (true) {}};</script>
            </body></html>'''
        else:
            body = b"<html><body>Recovered<script>addEventListener('pageshow', () => fetch('/recovered'))</script></body></html>"
        self.send_response(200)
        self.send_header('Content-Type', 'text/html')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


def wait(predicate, message, timeout=8):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return
        for _ in range(32):
            if not Gtk.events_pending():
                break
            Gtk.main_iteration_do(False)
        time.sleep(.02)
    raise AssertionError(message)


def xd(*arguments):
    return subprocess.check_output(['xdotool', *map(str, arguments)], text=True, timeout=5).strip()


def click(window, x, y):
    xd('mousemove', '--window', window, x, y)
    xd('click', 1)


def navigate(window, address):
    click(window, 400, 24)
    xd('key', 'ctrl+a')
    xd('type', '--clearmodifiers', '--delay', 5, address)
    xd('key', 'Return')


def hang(window):
    hung.clear()
    click(window, 50, 68)
    wait(hung.is_set, 'page did not start its infinite JavaScript loop')
    time.sleep(.5)


server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
threading.Thread(target=server.serve_forever, daemon=True).start()
base = f'http://127.0.0.1:{server.server_port}'
child = None
try:
    with tempfile.TemporaryDirectory(prefix='browser-responsiveness-', dir='target') as folder:
        root = Path(folder).resolve()
        title = f'Bokheim browser responsiveness {os.getpid()}'
        request = {'url': base + '/hang', 'title': title,
                   'data_directory': str(root / 'profile'), 'downloads': str(root / 'downloads')}
        with (root / 'helper.log').open('w') as log:
            try:
                child = subprocess.Popen([args.helper, 'store'], stdin=subprocess.PIPE,
                                         stdout=subprocess.PIPE, stderr=log)
                child.stdin.write((json.dumps(request) + '\n').encode())
                child.stdin.flush()
                assert select.select([child.stdout], [], [], 25)[0], 'browser did not open'
                assert json.loads(child.stdout.readline())['kind'] == 'opened'
                wait(loaded.is_set, 'hang fixture did not load')
                window = xd('search', '--onlyvisible', '--name', '^' + title + '$').splitlines()[0]
                xd('windowfocus', '--sync', window)
                hang(window)

                xd('mousemove', '--window', window, 400, 24)
                xd('click', 1)
                xd('key', 'ctrl+a')
                xd('type', '--clearmodifiers', '--delay', 10, 'controls still respond')
                xd('key', 'ctrl+a')
                xd('key', 'ctrl+c')
                copied = []
                Gtk.Clipboard.get(Gdk.SELECTION_CLIPBOARD).request_text(lambda _, text, *unused: copied.append(text))
                wait(lambda: copied, 'address bar could not copy text while the page was hung')
                assert copied == ['controls still respond'], copied
                print('Address bar accepts and copies input while page JavaScript is hung', flush=True)

                xd('type', '--clearmodifiers', '--delay', 5, base + '/recover')
                xd('key', 'Return')
                wait(recovered.is_set, 'address navigation did not recover from the hung page', timeout=15)
                print('Address navigation recovers from the hung page', flush=True)
                # The page reports pageshow before GPUI consumes the native
                # history update and enables its previously disabled Back button.
                time.sleep(.5)

                # History traversal must keep its target across a restart.
                loaded.clear()
                click(window, 24, 24)
                wait(loaded.is_set, 'Back did not return to the hang fixture')
                hang(window)
                recovered.clear()
                click(window, 64, 24)
                wait(recovered.is_set, 'Forward did not recover from the hung page', timeout=15)
                print('Forward recovers from the hung page', flush=True)

                loaded.clear()
                navigate(window, base + '/hang')
                wait(loaded.is_set, 'hang fixture did not load again')
                hang(window)
                recovered.clear()
                click(window, 24, 24)
                wait(recovered.is_set, 'Back did not recover from the hung page', timeout=15)
                print('Back recovers from the hung page', flush=True)

                loaded.clear()
                navigate(window, base + '/hang')
                wait(loaded.is_set, 'reload fixture did not load')
                hang(window)
                loaded.clear()
                click(window, 104, 24)
                wait(loaded.is_set, 'Reload did not recover from the hung page', timeout=15)
                print('Reload recovers from the hung page', flush=True)
                hang(window)
                child.stdin.close()
                assert child.wait(timeout=8) == 0, 'browser could not close'
                print('Browser closes while page JavaScript is hung', flush=True)
            except:
                log.flush()
                print((root / 'helper.log').read_text(), flush=True)
                raise
finally:
    if child and child.poll() is None:
        child.kill()
        child.wait()
    server.shutdown()
