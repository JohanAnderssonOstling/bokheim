"""Exercise native GPUI/WebKit focus on an isolated X11 test display.

Run from the workspace root after building opds-webview:
    xvfb-run -a python3 apps/opds-webview/tests/native_focus.py --display-env
Requires Python GTK 3 bindings and xdotool. Never run against a user session.
"""
import argparse
import json, os, select, subprocess, tempfile, threading, time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlparse
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--display', help='An isolated X11 test display')
parser.add_argument('--display-env', action='store_true', help='Use DISPLAY provided by xvfb-run')
parser.add_argument('--helper', default='target/release/bokheim-opds-webview')
args = parser.parse_args()
if not args.display and not args.display_env:
    parser.error('specify an isolated --display or use xvfb-run with --display-env')
if args.display:
    os.environ['DISPLAY'] = args.display
os.environ['GDK_BACKEND'] = 'x11'
os.environ.pop('WAYLAND_DISPLAY', None)
import gi
gi.require_version('Gtk','3.0')
from gi.repository import Gtk, Gdk
Gtk.init([])
states=[]
class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path.startswith('/state?'):
            states.append(parse_qs(urlparse(self.path).query)); body=b'ok'
        else:
            body=b'''<html><body><input id="entry" style="position:absolute;left:10px;top:10px;width:300px"><script>setInterval(()=>fetch('/state?focus='+document.hasFocus()+'&value='+encodeURIComponent(entry.value)+'&width='+innerWidth+'&height='+innerHeight),100)</script></body></html>'''
        self.send_response(200); self.send_header('Content-Length', str(len(body))); self.end_headers(); self.wfile.write(body)
    def log_message(self,*args): pass
server=ThreadingHTTPServer(('127.0.0.1',0),Handler)
threading.Thread(target=server.serve_forever,daemon=True).start()
def xd(*args): return subprocess.check_output(['xdotool',*map(str,args)],text=True).strip()
def wait_focus(expected, value=None):
    deadline=time.monotonic()+5
    previous=len(states)
    while time.monotonic()<deadline:
        if len(states)>previous and states[-1]['focus']==[str(expected).lower()] and (value is None or states[-1].get('value',[''])==[value]): return
        time.sleep(.1)
    raise AssertionError((expected,value,states[-5:]))
process=None
with tempfile.TemporaryDirectory(prefix='browser-focus-',dir='target') as folder:
    root=Path(folder).resolve()
    with (root/'helper.log').open('w') as log:
        try:
            request={'url':f'http://127.0.0.1:{server.server_port}/','title':f'Bokheim native focus regression {os.getpid()}','data_directory':str(root/'profile'),'downloads':str(root/'downloads')}
            process=subprocess.Popen([args.helper,'store'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=log)
            process.stdin.write((json.dumps(request)+'\n').encode()); process.stdin.flush()
            assert select.select([process.stdout],[],[],20)[0], 'helper did not open'
            assert json.loads(process.stdout.readline())['kind']=='opened'
            time.sleep(1)
            window=xd('search','--onlyvisible','--name',f'^Bokheim native focus regression {os.getpid()}$').splitlines()[0]
            xd('windowfocus','--sync',window)
            wait_focus(True)
            print('Direct page launch receives native focus',flush=True)
            xd('mousemove','--window',window,400,28); xd('click',1)
            wait_focus(False)
            xd('mousemove','--window',window,100,68); xd('click',1)
            wait_focus(True)
            xd('type','--clearmodifiers','--delay',20,'native page input')
            wait_focus(True,'native page input')
            print('Page click and native keyboard input passed',flush=True)
            xd('mousemove','--window',window,400,28); xd('click',1)
            wait_focus(False,'native page input')
            xd('key','ctrl+a'); xd('type','--clearmodifiers','--delay',10,'address editing')
            wait_focus(False,'native page input')
            xd('key','ctrl+a'); xd('key','ctrl+c'); time.sleep(.1)
            assert Gtk.Clipboard.get(Gdk.SELECTION_CLIPBOARD).wait_for_text() == 'address editing'
            xd('mousemove','--window',window,100,68); xd('click',1)
            wait_focus(True,'native page input')
            xd('key','End')
            xd('type','--clearmodifiers','--delay',10,' again')
            wait_focus(True,'native page input again')
            print('Address bar and repeated page focus passed',flush=True)
            other=Gtk.Window(title=f'Bokheim native focus other window {os.getpid()}'); other.show_all()
            while Gtk.events_pending(): Gtk.main_iteration_do(False)
            other_id=xd('search','--onlyvisible','--name',f'^Bokheim native focus other window {os.getpid()}$').splitlines()[0]
            xd('windowfocus','--sync',other_id)
            wait_focus(False,'native page input again')
            xd('windowfocus','--sync',window)
            wait_focus(True,'native page input again')
            xd('type','--clearmodifiers','--delay',10,' restored')
            wait_focus(True,'native page input again restored')
            print('Window deactivation and page keyboard restoration passed',flush=True)
            xd('mousemove','--window',window,400,28); xd('click',1)
            wait_focus(False,'native page input again restored')
            xd('windowfocus','--sync',other_id); time.sleep(.2)
            xd('windowfocus','--sync',window)
            wait_focus(False,'native page input again restored')
            print('Address bar selection survives window switching',flush=True)
            xd('key','Escape')
            wait_focus(True,'native page input again restored')
            print('Escape returns native focus to the page',flush=True)
            xd('mousemove','--window',window,400,28); xd('click',1)
            wait_focus(False,'native page input again restored')
            xd('key','ctrl+a')
            xd('type','--clearmodifiers','--delay',5,f'http://127.0.0.1:{server.server_port}/next')
            xd('key','Return')
            wait_focus(True,'')
            print('URL submission focuses the new page',flush=True)
            assert states[-1]['width'] == ['1100'] and states[-1]['height'] == ['752'], states[-1]
            xd('windowsize',window,980,640)
            deadline=time.monotonic()+5
            while time.monotonic()<deadline:
                if states[-1]['width']==['980'] and states[-1]['height']==['592']: break
                time.sleep(.1)
            assert states[-1]['width']==['980'] and states[-1]['height']==['592'],states[-1]
            print('Native browser surface fits the viewport and follows resizing',flush=True)
            other.destroy()
            process.stdin.close(); process.wait(timeout=10); assert process.returncode==0
            print('All native focus regressions passed',flush=True)
        except:
            log.flush(); print((root/'helper.log').read_text(),flush=True); raise
        finally:
            if process and process.poll() is None: process.kill(); process.wait()
server.shutdown()
