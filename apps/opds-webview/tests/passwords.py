#!/usr/bin/env python3
"""Automatic password regression using a private display and system keyring.

Requires Xvfb, gnome-keyring-daemon, secret-tool and xdotool.
No credentials or windows in the user's desktop session are accessed.
"""
import argparse
import json
import os
import select
import subprocess
import sys
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlparse

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--helper', default='target/debug/bokheim-opds-webview')
parser.add_argument('--xvfb', default='Xvfb')
parser.add_argument('--keyring-daemon', default='gnome-keyring-daemon')
parser.add_argument('--artifacts', default='target/password-manager-tests')
parser.add_argument('--private-bus', action='store_true', help=argparse.SUPPRESS)
args = parser.parse_args()
if not args.private_bus:
    raise SystemExit(subprocess.call(['dbus-run-session', '--', sys.executable, __file__, *sys.argv[1:], '--private-bus']))

artifacts = Path(args.artifacts).resolve()
artifacts.mkdir(parents=True, exist_ok=True)
states = {}
submissions = []


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        query = parse_qs(urlparse(self.path).query, keep_blank_values=True)
        stage = query.get('stage', ['first'])[0]
        if self.path.startswith('/state'):
            states[stage] = query
            body = 'ok'
        else:
            mode = query.get('mode', ['empty'])[0]
            username, password = {
                'save': ('fixture-reader', 'fixture-secret-123'),
                'typed': ('typed-user', 'typed-secret'),
                'failed': ('fixture-reader', 'rejected-secret-456'),
                'update': ('fixture-reader', 'updated-secret-789')
            }.get(mode, ('', ''))
            form = f'''<form method="post" action="/signed-in?stage={stage}&amp;failed={int(mode == "failed")}">
                <label>Username<input id="user" name="username" autocomplete="username" value="{username}"></label>
                <label>Password<input id="pass" name="password" autocomplete="current-password" type="password" value="{password}"></label>
                <button style="position:absolute;left:16px;top:120px;width:160px;height:40px">Sign in</button></form>'''
            body = '<!doctype html><html><body style="position:relative;background:#dbe9d1">'
            body += ('<script>setTimeout(()=>{document.body.insertAdjacentHTML("afterbegin",' + json.dumps(form) + ')},500)</script>') if mode == 'dynamic' else form
            body += f'''<script>setInterval(()=>fetch('/state?stage={stage}&user='+encodeURIComponent(document.querySelector('#user')?.value || '')+
                '&password='+encodeURIComponent(document.querySelector('#pass')?.value || '')+
                '&ready='+Boolean(window.__bokheimPasswords)+'&height='+innerHeight+'&phase={query.get('phase', ['initial'])[0]}'),100);</script></body></html>'''
        self.reply(body)

    def do_POST(self):
        self.rfile.read(int(self.headers.get('Content-Length', 0)))
        submissions.append(self.path)
        query = parse_qs(urlparse(self.path).query)
        if query.get('failed') == ['1']:
            self.path = '/?stage=' + query['stage'][0] + '&mode=empty&phase=failed'
            self.do_GET()
        else:
            self.reply('<!doctype html><p>Signed in</p>')

    def reply(self, body):
        data = body.encode()
        self.send_response(200)
        self.send_header('Content-Type', 'text/html')
        self.send_header('Cache-Control', 'no-store')
        self.send_header('Content-Length', str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, *_):
        pass


def wait(predicate, message, timeout=15):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(.1)
    raise AssertionError(message)


server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
threading.Thread(target=server.serve_forever, daemon=True).start()
children = []
try:
    with tempfile.TemporaryDirectory(prefix='password-manager-', dir='target') as folder:
        root = Path(folder).resolve()
        env = os.environ.copy()
        env.pop('WAYLAND_DISPLAY', None)
        env.update(GDK_BACKEND='x11', NO_AT_BRIDGE='1', GTK_USE_PORTAL='0',
                   XDG_DATA_HOME=str(root / 'xdg-data'), XDG_CACHE_HOME=str(root / 'xdg-cache'))
        with (artifacts / 'helper.log').open('w') as log:
            display = subprocess.Popen([args.xvfb, '-displayfd', '1', '-screen', '0', '1280x900x24', '-nolisten', 'tcp', '-ac'],
                                       stdout=subprocess.PIPE, stderr=log, env=env)
            children.append(display)
            assert select.select([display.stdout], [], [], 10)[0], 'private display did not start'
            env['DISPLAY'] = ':' + display.stdout.readline().decode().strip()
            daemon = subprocess.Popen([args.keyring_daemon, '--foreground', '--components=secrets', '--unlock',
                                       '--control-directory', str(root / 'keyring-control')],
                                      stdin=subprocess.PIPE, stdout=log, stderr=log, env=env)
            children.append(daemon)
            daemon.stdin.write(b'synthetic-test-keyring-password')
            daemon.stdin.close()
            wait(lambda: (root / 'xdg-data/keyrings').exists(), 'private keyring did not start')

            def xd(*arguments):
                return subprocess.check_output(['xdotool', *map(str, arguments)], text=True, env=env).strip()

            def launch(stage, mode='empty', host='127.0.0.1'):
                child = subprocess.Popen([args.helper, 'store'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log, env=env)
                children.append(child)
                title = 'Bokheim password regression ' + stage
                request = {'url': f'http://{host}:{server.server_port}/?stage={stage}&mode={mode}', 'title': title,
                           'data_directory': str(root / 'profile'), 'downloads': str(root / 'downloads')}
                child.stdin.write((json.dumps(request) + '\n').encode())
                child.stdin.flush()
                assert select.select([child.stdout], [], [], 25)[0], 'browser did not start'
                assert json.loads(child.stdout.readline())['kind'] == 'opened'
                wait(lambda: states.get(stage, {}).get('ready') == ['true'], 'autofill did not initialize')
                window = xd('search', '--onlyvisible', '--name', '^' + title + '$').splitlines()[0]
                return child, window

            def close(child):
                child.stdin.close()
                assert child.wait(timeout=15) == 0

            first, window = launch('save', mode='save')
            geometry = dict(line.split('=', 1) for line in xd('getwindowgeometry', '--shell', window).splitlines())
            page_top = int(geometry['HEIGHT']) - int(states['save']['height'][0])
            xd('windowfocus', '--sync', window)
            xd('mousemove', '--window', window, 90, page_top + 148)
            xd('click', 1)
            wait(lambda: len(submissions) == 1, 'real sign-in form was not submitted')
            index = root / 'profile/passwords.json'
            wait(index.exists, 'submitted login was not saved')
            assert json.loads(index.read_text()) == [{'origin': f'http://127.0.0.1:{server.server_port}', 'username': 'fixture-reader'}]
            assert 'fixture-secret' not in index.read_text()
            stored = subprocess.run(['secret-tool', 'search', '--all', 'service', 'se.bokheim.browser.passwords'],
                                    capture_output=True, env=env, check=False)
            assert b'fixture-secret-123' in stored.stdout, 'password was not saved in the isolated keyring'
            close(first)
            print('Real form submission automatically saves the login in the local system keyring', flush=True)

            for stage, mode in [('relaunch', 'empty'), ('dynamic', 'dynamic')]:
                child, _ = launch(stage, mode=mode)
                wait(lambda: states[stage]['password'] == ['fixture-secret-123'], 'saved login was not automatically filled: ' + stage)
                assert states[stage]['user'] == ['fixture-reader']
                assert len(submissions) == 1, 'autofill submitted the form'
                close(child)
            print('Automatic filling survives relaunch and detects dynamically inserted forms without UI', flush=True)

            child, _ = launch('foreign', host='localhost')
            time.sleep(.5)
            assert states['foreign']['password'] == [''], 'another origin received a password'
            close(child)
            child, _ = launch('typed', mode='typed')
            time.sleep(.5)
            assert states['typed']['password'] == ['typed-secret']
            assert states['typed']['user'] == ['typed-user']
            close(child)
            assert len(submissions) == 1
            assert json.loads(index.read_text())[0]['username'] == 'fixture-reader'
            print('Other origins and existing form values are left alone; no automatic form submission', flush=True)

            def submit(window, stage):
                geometry = dict(line.split('=', 1) for line in xd('getwindowgeometry', '--shell', window).splitlines())
                page_top = int(geometry['HEIGHT']) - int(states[stage]['height'][0])
                xd('windowfocus', '--sync', window)
                xd('mousemove', '--window', window, 90, page_top + 148)
                xd('click', 1)

            def stored_password():
                return subprocess.run(['secret-tool', 'search', '--all', 'service', 'se.bokheim.browser.passwords'],
                                      capture_output=True, env=env, check=False).stdout

            child, window = launch('failed', mode='failed')
            submit(window, 'failed')
            wait(lambda: states['failed'].get('phase') == ['failed'], 'failed-login response did not load')
            time.sleep(1)
            assert b'fixture-secret-123' in stored_password()
            assert b'rejected-secret-456' not in stored_password(), 'failed login replaced the saved password'
            close(child)
            print('A failed login that keeps the form preserves the previously saved password', flush=True)

            child, window = launch('update', mode='update')
            submit(window, 'update')
            wait(lambda: b'updated-secret-789' in stored_password(), 'successful changed password was not saved')
            assert b'fixture-secret-123' not in stored_password()
            close(child)
            child, _ = launch('updated-relaunch')
            wait(lambda: states['updated-relaunch']['password'] == ['updated-secret-789'], 'updated password was not filled on relaunch')
            close(child)
            print('Form disappearance commits a password update, which is filled on the next visit', flush=True)
finally:
    server.shutdown()
    for child in reversed(children):
        if child.poll() is None:
            child.terminate()
            try:
                child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait()
