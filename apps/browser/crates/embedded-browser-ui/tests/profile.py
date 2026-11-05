#!/usr/bin/env python3
"""Real browser cookie and website-storage persistence regression.

After building the helper, run on an isolated display:
    xvfb-run -a python3 apps/browser/crates/embedded-browser-ui/tests/profile.py --display-env
"""
import argparse
import json
import os
import select
import sqlite3
import subprocess
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlparse

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--display', help='An isolated X11 test display')
parser.add_argument('--display-env', action='store_true', help='Use DISPLAY supplied by xvfb-run')
parser.add_argument('--helper', default='target/release/bokheim-browser')
args = parser.parse_args()
if not args.display and not args.display_env:
    parser.error('specify an isolated --display or use xvfb-run with --display-env')
env = os.environ.copy()
if args.display:
    env['DISPLAY'] = args.display
env['GDK_BACKEND'] = 'x11'
env.pop('WAYLAND_DISPLAY', None)
seen = {}
change_first = threading.Event()


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        query = parse_qs(urlparse(self.path).query)
        stage = query.get('stage', [''])[0]
        path = urlparse(self.path).path
        reporting = path == '/seen'
        late = path == '/command' and stage == 'first' and change_first.is_set()
        if late:
            change_first.clear()
        if reporting:
            seen[stage] = (self.headers.get('Cookie', ''), query.get('previous', [''])[0])
            body = b'ok'
        elif path == '/command':
            body = b'changed' if late else b'wait'
        else:
            body = ("<script>const previous = localStorage.getItem('remembered') || '';"
                    "localStorage.setItem('remembered', 'saved');"
                    f"fetch('/seen?stage={stage}&previous='+encodeURIComponent(previous));"
                    f"setInterval(()=>fetch('/command?stage={stage}').then(r=>r.text()).then(t=>{{"
                    "if(t==='changed') fetch('/seen?stage=late')}),100);</script>").encode()
        self.send_response(200)
        if path == '/' and stage in ('first', 'second'):
            self.send_header('Set-Cookie', f'{stage}=retained; Path=/; Max-Age=3600; HttpOnly; SameSite=Lax')
        if late:
            self.send_header('Set-Cookie', 'late=retained; Path=/; Max-Age=3600; HttpOnly; SameSite=Lax')
        self.send_header('Content-Type', 'text/html')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
threading.Thread(target=server.serve_forever, daemon=True).start()
processes = []


def launch(root, profile, stage, log):
    request = {'url': f'http://127.0.0.1:{server.server_port}/?stage={stage}',
               'title': f'Bokheim profile regression {stage}', 'data_directory': str(profile),
               'downloads': str(root / 'downloads'), 'history_file': str(root / 'history.json'), 'preferences': {'keyboard_shortcuts': {}, 'allow_metered_transfers': False, 'text_search': True, 'animations': True, 'custom_ui_font_enabled': False, 'generated_covers_only': False, 'catalog_grouping': 'subject', 'keyboard_layouts': ['SV'], 'ui_font_family': 'serif', 'theme': 'cool', 'ui_font_size_px': 16, 'cover_text': 'always'}}
    child = subprocess.Popen([args.helper, 'browser'], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                             stderr=log, env=env)
    processes.append(child)
    child.stdin.write((json.dumps(request) + '\n').encode())
    child.stdin.flush()
    assert select.select([child.stdout], [], [], 25)[0], 'browser did not open'
    assert json.loads(child.stdout.readline())['kind'] == 'opened'
    deadline = time.monotonic() + 10
    while stage not in seen and time.monotonic() < deadline:
        time.sleep(.05)
    assert stage in seen, f'{stage} page did not load'
    return child


def close(child):
    child.stdin.close()
    assert child.wait(timeout=10) == 0


def wait_for_storage(root):
    # WebKit batches local-storage writes. Wait for its disk flush before
    # opening another process, instead of relying on machine-dependent timing.
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        for file in root.rglob('*.localstorage'):
            try:
                with sqlite3.connect(f'file:{file}?mode=ro', uri=True) as database:
                    if database.execute("SELECT count(*) FROM ItemTable WHERE key = 'remembered'").fetchone()[0]:
                        return
            except sqlite3.Error:
                pass
        time.sleep(.05)
    raise AssertionError('WebKit did not persist the fixture local storage')


try:
    with tempfile.TemporaryDirectory(prefix='browser-profile-', dir='target') as folder:
        root = Path(folder).resolve()
        profile = root / 'profile'
        with (root / 'helper.log').open('w') as log:
            first = launch(root, profile, 'first', log)
            wait_for_storage(root)
            second = launch(root, profile, 'second', log)
            assert 'first=retained' in seen['second'][0], seen['second']
            assert seen['second'][1] == 'saved', seen['second']
            # The older window writes after the newer one has saved a cookie.
            # A whole-file text jar would erase the newer window's cookie.
            change_first.set()
            deadline = time.monotonic() + 10
            while 'late' not in seen and time.monotonic() < deadline:
                time.sleep(.05)
            assert 'late' in seen and 'late=retained' in seen['late'][0], seen
            close(second)
            close(first)
            print('Concurrent browser windows share cookies and local storage', flush=True)
            third = launch(root, profile, 'relaunch', log)
            for cookie in ('first=retained', 'second=retained', 'late=retained'):
                assert cookie in seen['relaunch'][0], seen['relaunch']
            assert seen['relaunch'][1] == 'saved', seen['relaunch']
            close(third)
            databases = list(profile.glob('cookies.sqlite'))
            assert len(databases) == 1, databases
            print('Cookies survive all helper exits and reopening from another entry point', flush=True)
finally:
    for child in processes:
        if child.poll() is None:
            child.kill()
            child.wait()
    server.shutdown()
