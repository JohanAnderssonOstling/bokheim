#!/usr/bin/env python3
"""Exercise automatic password filling against real browser documents.

Requires Python Playwright and its Chromium browser:
    python apps/opds-webview/tests/password_forms.py
"""
import argparse
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from threading import Thread
from playwright.sync_api import sync_playwright

SCRIPT = (Path(__file__).resolve().parents[1] / 'src/passwords.js').read_text()
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--chromium', help='Optional path to a Chromium executable')
args = parser.parse_args()
FORM = '''<!doctype html><form onsubmit="window.submitted=true;return false">
    <label>Email<input id="user" name="username" autocomplete="username"></label>
    <label>Password<input id="pass" name="password" type="password" autocomplete="current-password"></label>
    <button>Sign in</button></form>'''


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        body = FORM
        if self.path == '/foreign-form':
            body = body.replace('<form ', '<form action="https://other.example/login" ')
        elif self.path == '/new-password':
            body = body.replace('current-password', 'new-password')
        elif self.path == '/multiple':
            body = body.replace(' autocomplete="current-password"', '').replace('</form>', '<input type="password"></form>')
        elif self.path == '/readonly':
            body = body.replace('id="user"', 'id="user" readonly value="another-account"')
        elif self.path in ['/hidden', '/invisible', '/transparent']:
            style = {'/hidden': 'display:none', '/invisible': 'visibility:hidden', '/transparent': 'opacity:0'}[self.path]
            body = body.replace('id="pass"', f'id="pass" style="{style}"')
        elif self.path == '/mutate':
            body += '''<script>document.querySelector('#user').addEventListener('input', () => {
                document.querySelector('form').action = 'https://other.example/login';
            });</script>'''
        elif self.path == '/frame':
            body = f'<iframe src="http://localhost:{self.server.server_port}/login"></iframe>'
        elif self.path == '/dynamic':
            body = '<!doctype html><script>setTimeout(()=>{document.body.innerHTML=' + json.dumps(FORM) + '},250)</script>'
        body += '''<script>window.changes=[];
            document.addEventListener('input', event => changes.push(event.target.id));
            document.addEventListener('change', event => changes.push('change:'+event.target.id));</script>'''
        data = body.encode()
        self.send_response(200)
        self.send_header('Content-Type', 'text/html')
        self.send_header('Content-Length', str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, *_):
        pass


server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
Thread(target=server.serve_forever, daemon=True).start()
origin = f'http://127.0.0.1:{server.server_port}'
request = {'kind': 'fill', 'nonce': 'current-document', 'origin': origin,
           'username': 'reader@example.test', 'password': 'quotes"\\n</script> — secret'}
try:
    with sync_playwright() as playwright:
        browser = playwright.chromium.launch(headless=True, executable_path=args.chromium)
        page = browser.new_page()
        page.add_init_script('window.messages=[];window.ipc={postMessage: message=>messages.push(JSON.parse(message))}')

        def install(path='/login'):
            page.goto(origin + path)
            assert page.evaluate(SCRIPT, {'kind': 'install', 'nonce': request['nonce'], 'origin': origin}) is True

        install()
        page.wait_for_function("messages.some(message=>message.kind==='fill')")
        assert set(page.evaluate('messages[0]')) == {'nonce', 'origin', 'kind', 'username'}
        focus = page.evaluate('document.activeElement.id')
        assert page.evaluate(SCRIPT, request) is True
        assert page.locator('#user').input_value() == request['username']
        assert page.locator('#pass').input_value() == request['password']
        assert page.evaluate('window.changes') == ['user', 'change:user', 'pass', 'change:pass']
        assert page.evaluate('document.activeElement.id') == focus
        assert page.evaluate('Boolean(window.submitted)') is False
        print('Automatic lookup, field events, preserved focus and no automatic submission passed')

        for path in ['/foreign-form', '/new-password', '/multiple', '/readonly', '/hidden', '/invisible', '/transparent', '/mutate']:
            install(path)
            assert page.evaluate(SCRIPT, request) is None, path
            assert page.locator('#pass').input_value() == '', path
        print('Cross-origin actions, new passwords, ambiguous/hidden fields and changed form actions blocked')

        install()
        page.locator('#pass').fill('typed-secret')
        assert page.evaluate(SCRIPT, request) is None
        assert page.locator('#pass').input_value() == 'typed-secret'
        page.locator('#pass').fill('')
        assert page.evaluate(SCRIPT, request) is None, 'clearing a password must not immediately refill it'
        install()
        page.locator('#user').fill('another-account')
        assert page.evaluate(SCRIPT, request) is None
        for other in [f'http://localhost:{server.server_port}', 'http://127.0.0.1:1', 'https://127.0.0.1']:
            assert page.evaluate(SCRIPT, {**request, 'origin': other}) is None
        assert page.evaluate(SCRIPT, {**request, 'nonce': 'old-document'}) is None
        assert page.locator('#pass').input_value() == ''
        print('Typed credentials, cleared passwords, scheme/host/port and stale document protection passed')

        install('/frame')
        assert page.evaluate(SCRIPT, request) is None
        frame = page.frames[1]
        assert frame.evaluate(SCRIPT, {**request, 'origin': f'http://localhost:{server.server_port}'}) is None
        assert frame.locator('#pass').input_value() == ''
        install('/dynamic')
        page.wait_for_function("messages.some(message=>message.kind==='fill')")
        assert page.evaluate(SCRIPT, request) is True
        assert page.locator('#pass').input_value() == request['password']
        print('Cross-origin frames blocked; dynamically inserted forms request autofill')

        install()
        page.locator('#user').fill(request['username'])
        page.locator('#pass').fill(request['password'])
        page.dispatch_event('form', 'submit')
        assert not page.evaluate("messages.some(message=>message.kind==='submitted')")
        page.locator('button').click()
        submitted = page.evaluate("messages.find(message=>message.kind==='submitted')")
        assert submitted['password'] == request['password']
        assert submitted['username'] == request['username']
        assert submitted['origin'] == origin and submitted['nonce'] == request['nonce']
        page.wait_for_timeout(850)
        assert not page.evaluate("messages.some(message=>message.kind==='form_gone')"), 'a visible form must keep the password pending'
        page.evaluate("document.querySelector('form').remove()")
        page.wait_for_function("messages.some(message=>message.kind==='form_gone')")
        assert page.evaluate("messages.find(message=>message.kind==='form_gone').attempt") == submitted['attempt']
        print('Submission stays pending while the form is visible; disappearance confirms the matching attempt without sign-out controls')

        install()
        page.locator('#user').fill(request['username'])
        page.locator('#pass').fill(request['password'])
        page.locator('button').click()
        page.evaluate("document.querySelector('form').style.display='none'; setTimeout(()=>document.querySelector('form').style.display='', 150)")
        page.wait_for_timeout(1000)
        assert not page.evaluate("messages.some(message=>message.kind==='form_gone')"), 'a brief rendering change must not confirm success'
        print('Temporary hide/show leaves the submitted password pending')
        browser.close()
finally:
    server.shutdown()
