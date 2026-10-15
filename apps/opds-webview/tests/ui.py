#!/usr/bin/env python3
"""Browser toolbar, native dropdown, theme, and download-toast and navigation regression.

Run after building the helper, on an isolated display:
    xvfb-run -a python3 apps/opds-webview/tests/ui.py --display-env
Requires xdotool, ffmpeg, tesseract, and Python Pillow.
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
from urllib.parse import parse_qs, urlparse
from PIL import Image, ImageChops, ImageOps, ImageStat

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--display', help='An isolated X11 test display')
parser.add_argument('--display-env', action='store_true', help='Use DISPLAY supplied by xvfb-run')
parser.add_argument('--helper', default='target/release/bokheim-opds-webview')
parser.add_argument('--appearance', choices=('dark', 'light'), default='dark')
args = parser.parse_args()
if not args.display and not args.display_env:
    parser.error('specify an isolated --display or use xvfb-run with --display-env')
env = os.environ.copy()
if args.display:
    env['DISPLAY'] = args.display
env['GDK_BACKEND'] = 'x11'
env.pop('WAYLAND_DISPLAY', None)
states = []
release_download = threading.Event()
download_requested = threading.Event()
artifacts = Path('target/browser-ui-regression') / args.appearance
artifacts.mkdir(parents=True, exist_ok=True)


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        path = urlparse(self.path).path
        if path == '/state':
            states.append(parse_qs(urlparse(self.path).query))
            body = b'ok'
        elif path in ('/slow.pdf', '/failed.pdf'):
            body = b'%PDF-1.7\nA local browser download fixture\n%%EOF\n' + b'\n' * 8192
            self.send_response(200)
            self.send_header('Content-Type', 'application/pdf')
            self.send_header('Content-Disposition', f'attachment; filename="{path[1:]}"')
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body[:1024])
            self.wfile.flush()
            if path == '/slow.pdf':
                download_requested.set()
            if path == '/slow.pdf':
                release_download.wait(timeout=20)
            self.wfile.write(body[1024:])
            return
        else:
            body = b'''<html><body style="background:#c2dfb3">
            <a href="/slow.pdf" style="position:absolute;left:10px;top:50px">Download slow book</a>
            <a href="/failed.pdf" style="position:absolute;left:10px;top:100px">Download broken book</a>
            <script>setInterval(()=>fetch('/state?height='+innerHeight+'&path='+location.pathname+'&dark='+matchMedia('(prefers-color-scheme: dark)').matches),100)</script>
            </body></html>'''
        self.send_response(200)
        self.send_header('Content-Type', 'text/html')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


def wait(predicate, message, timeout=10):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(.1)
    raise AssertionError(message)


def xd(*arguments):
    return subprocess.check_output(['xdotool', *map(str, arguments)], text=True, env=env).strip()


def capture(window, name):
    geometry = dict(line.split('=', 1) for line in xd('getwindowgeometry', '--shell', window).splitlines())
    image = artifacts / f'{name}.png'
    subprocess.run(['ffmpeg', '-y', '-f', 'x11grab', '-video_size', geometry['WIDTH'] + 'x' + geometry['HEIGHT'],
                    '-window_id', window, '-i', env['DISPLAY'], '-frames:v', '1', str(image)],
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True, env=env)
    controls = Image.open(image).convert('RGB').crop((0, 0, int(geometry['WIDTH']), 260))
    if max(controls.getpixel((3, 3))) < 40:
        controls = ImageOps.invert(controls)
    controls = controls.resize((controls.width * 2, controls.height * 2))
    ocr_image = artifacts / f'{name}-controls.png'
    controls.save(ocr_image)
    text = subprocess.check_output(['tesseract', str(ocr_image), 'stdout', '--psm', '6'], stderr=subprocess.DEVNULL, text=True)
    address = Image.open(image).convert('RGB').crop((132, 8, int(geometry['WIDTH']) - 10, 230))
    if max(Image.open(image).convert('RGB').getpixel((3, 3))) < 40:
        address = ImageOps.invert(address)
    address = address.convert('L').resize((address.width * 4, address.height * 4)).point(lambda value: 0 if value < 190 else 255)
    address_image = artifacts / f'{name}-address.png'
    address.save(address_image)
    text += subprocess.check_output(['tesseract', str(address_image), 'stdout', '--psm', '6'], stderr=subprocess.DEVNULL, text=True)
    toast = Image.open(image).convert('RGB').crop((630, 580, int(geometry['WIDTH']) - 30, int(geometry['HEIGHT']) - 20))
    if max(Image.open(image).convert('RGB').getpixel((3, 3))) < 40:
        toast = ImageOps.invert(toast)
    toast = toast.resize((toast.width * 3, toast.height * 3))
    toast_image = artifacts / f'{name}-toasts.png'
    toast.save(toast_image)
    text += subprocess.check_output(['tesseract', str(toast_image), 'stdout', '--psm', '11'], stderr=subprocess.DEVNULL, text=True)

    return image, text


def type_address(window, value):
    xd('windowfocus', '--sync', window)
    xd('mousemove', '--window', window, 400, 24)
    xd('click', 1)
    xd('key', 'ctrl+a')
    xd('type', '--clearmodifiers', '--delay', 10, value)
    time.sleep(.3)


# Check filenames by basename: OCR may confuse the last letter of a PDF extension.
def ocr_key(text):
    return ''.join(character for character in text.lower() if character.isalnum())


server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
threading.Thread(target=server.serve_forever, daemon=True).start()
base = f'http://127.0.0.1:{server.server_port}'
child = None
try:
    with tempfile.TemporaryDirectory(prefix='browser-ui-', dir='target') as folder:
        root = Path(folder).resolve()
        history = root / 'history.json'
        history.write_text(json.dumps([base + '/suggest-one', base + '/suggest-two', 'previous search words']))
        preferences = {'theme': 'cool', 'appearance_override': args.appearance, 'ui_font_size_px': 16,
                       'ui_font_family': 'sans_serif', 'cover_text': 'always'}
        request = {'url': base + '/start', 'title': 'Bokheim browser UI regression',
                   'data_directory': str(root / 'profile'), 'downloads': str(root / 'downloads'),
                   'history_file': str(history), 'preferences': preferences}
        with (artifacts / 'helper.log').open('w') as log:
            child = subprocess.Popen([args.helper, 'store'], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                     stderr=log, env=env)
            child.stdin.write((json.dumps(request) + '\n').encode())
            child.stdin.flush()
            assert select.select([child.stdout], [], [], 25)[0], 'browser did not open'
            assert json.loads(child.stdout.readline())['kind'] == 'opened'
            wait(lambda: states, 'fixture did not load')
            time.sleep(.5)
            window = xd('search', '--onlyvisible', '--name', '^Bokheim browser UI regression$').splitlines()[0]
            initial_height = states[-1]['height']
            image, text = capture(window, 'toolbar')
            pixel = Image.open(image).convert('RGB').getpixel((3, 3))
            assert (max(pixel) < 40 if args.appearance == 'dark' else min(pixel) > 180), 'launch theme was not applied'
            assert states[-1]['dark'] == [str(args.appearance == 'dark').lower()], states[-1]
            assert 'Open in browser' not in text and 'Go' not in text, text
            disabled_back = Image.open(image).convert('RGB').crop((16, 16, 32, 32))
            disabled_forward = Image.open(image).convert('RGB').crop((56, 16, 72, 32))
            xd('mousemove', '--window', window, 24, 24); xd('click', 1)
            xd('mousemove', '--window', window, 64, 24); xd('click', 1)
            time.sleep(.3)
            assert states[-1]['path'] == ['/start'], 'unavailable history navigation should do nothing'
            print('Launch theme and compact toolbar passed', flush=True)

            type_address(window, 'suggest')
            image, text = capture(window, 'suggestions')
            assert 'suggest-one' in text and 'suggest-two' in text, text
            assert states[-1]['height'] == initial_height, 'dropdown resized the webpage'
            xd('mousemove', '--window', window, 400, 58)
            xd('click', 1)
            wait(lambda: states[-1]['path'] == ['/suggest-one'], 'mouse suggestion selection failed')
            assert states[-1]['height'] == initial_height
            type_address(window, 'suggest')
            xd('key', 'Down'); xd('key', 'Down'); xd('key', 'Return')
            wait(lambda: states[-1]['path'] == ['/suggest-two'], 'keyboard suggestion selection failed')
            enabled_image, _ = capture(window, 'navigation-enabled')
            enabled_back = Image.open(enabled_image).convert('RGB').crop((16, 16, 32, 32))
            assert max(ImageStat.Stat(ImageChops.difference(disabled_back, enabled_back)).mean) > 5, 'Back did not visually enable'
            xd('mousemove', '--window', window, 24, 24); xd('click', 1)
            wait(lambda: states[-1]['path'] == ['/suggest-one'], 'Back button failed')
            enabled_image, _ = capture(window, 'navigation-forward')
            enabled_forward = Image.open(enabled_image).convert('RGB').crop((56, 16, 72, 32))
            assert max(ImageStat.Stat(ImageChops.difference(disabled_forward, enabled_forward)).mean) > 5, 'Forward did not visually enable'
            xd('mousemove', '--window', window, 64, 24); xd('click', 1)
            wait(lambda: states[-1]['path'] == ['/suggest-two'], 'Forward button failed')
            # Reload remains available and retains the current history entry.
            before_reload = len(states)
            xd('mousemove', '--window', window, 104, 24); xd('click', 1)
            wait(lambda: len(states) > before_reload + 3, 'Reload button failed')
            assert states[-1]['path'] == ['/suggest-two']
            print('Native history buttons enable and navigate correctly', flush=True)
            type_address(window, 'previous')
            assert 'previous search words' in json.loads(history.read_text()), history.read_text()
            _, text = capture(window, 'search-history')
            assert 'previous' in text and 'words' in text, text
            xd('key', 'Escape')
            print('Address dropdown supports prior searches, mouse and keyboard without resizing the page', flush=True)

            xd('mousemove', '--window', window, 80, 110); xd('click', 1)
            wait(download_requested.is_set, 'page link did not request the download')
            time.sleep(1)
            _, text = capture(window, 'download-started')
            assert 'downloadingslow' in ocr_key(text), text
            assert states[-1]['height'] == initial_height, 'download toast resized the webpage'
            type_address(window, 'suggest')
            _, text = capture(window, 'download-and-suggestions')
            assert 'suggest-one' in text and 'suggest-two' in text and 'downloadingslow' in ocr_key(text), text
            xd('key', 'Escape')
            release_download.set()
            assert select.select([child.stdout], [], [], 10)[0], 'finished download was not reported'
            event = json.loads(child.stdout.readline())
            assert event['kind'] == 'downloaded' and Path(event['path']).read_bytes().startswith(b'%PDF-'), event
            # The browser hands the downloaded book to the application before
            # completion. Simulate its feedback after the library import.
            child.stdin.write((json.dumps({'download': event['download'], 'name': Path(event['path']).name,
                                          'phase': 'completed', 'percent': None, 'message': None}) + '\n').encode())
            child.stdin.flush()
            time.sleep(.3)
            _, text = capture(window, 'download-completed')
            assert 'downloadcompleteslow' in ocr_key(text), text
            assert states[-1]['height'] == initial_height, 'completion toast resized the webpage'
            # Standard successful notifications dismiss themselves after 5 seconds.
            time.sleep(5.5)
            image, text = capture(window, 'download-autohidden')
            assert 'downloadcomplete' not in ocr_key(text), text
            assert Image.open(image).convert('RGB').getpixel((800, 750)) == (194, 223, 179), 'dismissed toast left a hole in the native page'
            # Use a real destination-write failure rather than relying on how
            # a particular WebKit version classifies a truncated HTTP body.
            (root / 'downloads').chmod(0o500)
            xd('mousemove', '--window', window, 80, 158); xd('click', 1)
            time.sleep(1)
            _, text = capture(window, 'download-failed')
            assert 'downloadfailedfailed' in ocr_key(text), text
            (root / 'downloads').chmod(0o700)
            time.sleep(5.5)
            _, text = capture(window, 'download-error-persistent')
            assert 'downloadfailedfailed' in ocr_key(text), text
            # Shared toast close control appears on hover in the upper-right corner.
            xd('mousemove', '--window', window, 1070, 730)
            time.sleep(.3)
            xd('click', 1)
            time.sleep(.5)
            image, text = capture(window, 'download-dismissed')
            assert 'downloadfailed' not in ocr_key(text), text
            assert Image.open(image).convert('RGB').getpixel((800, 750)) == (194, 223, 179), 'closing toast left a hole in the native page'
            assert states[-1]['height'] == initial_height
            print('Download toasts show start, completion, autohide, persistent errors and dismissal above the native page', flush=True)
            child.stdin.close()
            assert child.wait(timeout=10) == 0
finally:
    release_download.set()
    if child and child.poll() is None:
        child.kill()
        child.wait()
    server.shutdown()
