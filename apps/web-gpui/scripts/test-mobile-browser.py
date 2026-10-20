#!/usr/bin/env python3
"""Exercise the mobile_probe WASM fixture using the production GPUI web backend.

Build/package the fixture as documented in apps/web-gpui/README.md, then pass
its directory (containing mobile_probe.js and mobile_probe_bg.wasm).
Requires Python Playwright and Chromium; CHROMIUM_PATH overrides the executable.
"""
import argparse
import functools
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import threading

from playwright.sync_api import sync_playwright


def prepare(bundle: Path, wasm: Path):
    root = Path(__file__).resolve().parents[3]
    bindgen = os.environ.get("WASM_BINDGEN") or shutil.which("wasm-bindgen")
    if not bindgen:
        bindgen = str(root / "target/wasm-bindgen-cli/bin/wasm-bindgen")
    bundle.mkdir(parents=True, exist_ok=True)
    subprocess.run([bindgen, "--target", "web", "--out-dir", str(bundle), str(wasm)], check=True)
    template = (root / "apps/web-gpui/index.html").read_text()
    template = re.sub(r'<script type="module">.*?</script>', '<script type="module">import init from "./mobile_probe.js"; await init();</script>', template, flags=re.S)
    template = re.sub(r'<main id="startup-status".*?</main>', '', template, flags=re.S)
    (bundle / "index.html").write_text(template)


def run(bundle: Path, engine: str = "chromium"):
    class Handler(SimpleHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def end_headers(self):
            self.send_header("Cross-Origin-Opener-Policy", "same-origin")
            self.send_header("Cross-Origin-Embedder-Policy", "require-corp")
            super().end_headers()

    server = ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(Handler, directory=str(bundle)))
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    results = []
    try:
        with sync_playwright() as playwright:
            if engine == "chromium":
                browser = playwright.chromium.launch(
                    executable_path=os.environ.get("CHROMIUM_PATH", "/usr/bin/chromium"),
                    headless=True,
                    args=["--no-sandbox", "--enable-unsafe-swiftshader", "--use-angle=swiftshader"],
                )
            else:
                browser = playwright.webkit.launch(executable_path=os.environ.get("WEBKIT_PATH"), headless=True)
            for name, width, height, touch in [
                ("desktop", 1280, 800, False), ("narrow-pointer", 390, 844, False),
                ("phone", 390, 844, True), ("small-phone", 320, 568, True),
                ("tablet", 820, 1180, True),
            ]:
                context = browser.new_context(viewport={"width": width, "height": height}, has_touch=touch, is_mobile=touch, device_scale_factor=(3 if touch and width < 600 else 2 if touch else 1))
                print(f"Testing {engine}: {name}", file=sys.stderr, flush=True)
                page = context.new_page()
                errors = []
                page.on("pageerror", lambda error: errors.append(str(error)))
                def console(message):
                    if message.type == "error":
                        print(message.text, file=sys.stderr)
                        if message.text.startswith("[ERROR]"):
                            errors.append(message.text)
                page.on("console", console)
                page.goto(f"http://127.0.0.1:{server.server_port}/index.html")
                page.wait_for_function("typeof window.__BOKHEIM_MOBILE_PROBE__ === 'string'", timeout=120_000)
                try:
                    page.wait_for_function("JSON.parse(window.__BOKHEIM_MOBILE_PROBE__).width === innerWidth", timeout=10_000)
                except Exception:
                    print(page.evaluate("({state: window.__BOKHEIM_MOBILE_PROBE__, width: innerWidth, height: innerHeight, canvas: document.querySelector('canvas').getBoundingClientRect().toJSON()})"), errors, file=sys.stderr)
                    raise

                def state():
                    return page.evaluate("JSON.parse(window.__BOKHEIM_MOBILE_PROBE__)")

                page.evaluate("""() => {
                    globalThis.__probePointers = [];
                    for (const name of ['pointerdown', 'pointerup', 'pointercancel', 'lostpointercapture']) {
                        document.querySelector('canvas').addEventListener(name, event => __probePointers.push({type: name, id: event.pointerId, x: event.offsetX, y: event.offsetY, pointer: event.pointerType, timestamp: event.timeStamp, received: performance.now()}), {capture: true});
                    }
                }""")

                def until(expression):
                    try:
                        page.wait_for_function(expression, timeout=10_000)
                    except Exception:
                        print({"case": name, "state": state(), "pointers": page.evaluate("__probePointers"), "errors": errors}, file=sys.stderr)
                        page.screenshot(path=str(bundle.parent / f"failure-{engine}-{name}.png"))
                        raise

                initial = state()
                assert initial["touch"] == touch, initial
                assert initial["platformTouch"] == touch, initial
                assert initial["controlHeight"] == (48 if touch else 32), initial
                assert initial["compact"] == (width < 600), initial
                assert not initial["nativeBack"], initial
                if touch:
                    assert initial["readerTouch"], initial
                    session = context.new_cdp_session(page) if engine == "chromium" else None
                    active_contacts = {}

                    def contacts(kind, points):
                        if session:
                            session.send("Input.dispatchTouchEvent", {"type": kind, "touchPoints": [
                                {"x": x, "y": y, "id": index + 1} for index, (x, y) in enumerate(points)
                            ]})
                            return
                        # WebKit has no CDP touch stream. Inject PointerEvents
                        # through the actual production DOM event listeners.
                        incoming = {index + 1: point for index, point in enumerate(points)}
                        events = []
                        if kind in ("touchEnd", "touchCancel"):
                            events = [("pointerup" if kind == "touchEnd" else "pointercancel", key, *point) for key, point in active_contacts.items()]
                        else:
                            events = [("pointerdown" if key not in active_contacts else "pointermove", key, *point) for key, point in incoming.items()]
                        for event_type, pointer_id, x, y in events:
                            page.evaluate("""event => document.querySelector('canvas').dispatchEvent(new PointerEvent(event.type, {
                                bubbles: true, cancelable: true, pointerType: 'touch', pointerId: event.id,
                                clientX: event.x, clientY: event.y, button: 0, buttons: event.type === 'pointerup' ? 0 : 1,
                                pressure: 0.5, isPrimary: event.id === 1
                            }))""", {"type": event_type, "id": pointer_id, "x": x, "y": y})
                        active_contacts.clear()
                        active_contacts.update(incoming)

                    def tap(x, y):
                        if session:
                            session.send("Input.synthesizeTapGesture", {"x": x, "y": y, "duration": 50, "gestureSourceType": "touch"})
                        else:
                            contacts("touchStart", [(x, y)])
                            contacts("touchEnd", [])

                    def drag(x, y, dx, dy):
                        if session:
                            session.send("Input.synthesizeScrollGesture", {"x": x, "y": y, "xDistance": -dx, "yDistance": -dy, "speed": 1000, "gestureSourceType": "touch"})
                        else:
                            contacts("touchStart", [(x, y)])
                            contacts("touchMove", [(x + dx, y + dy)])
                            contacts("touchEnd", [])

                    tap(60, 40)
                    until("JSON.parse(window.__BOKHEIM_MOBILE_PROBE__).taps === 1")
                    assert page.locator("input").first.get_attribute("inputmode") == "none"
                    # Dragging from a button must never activate it on release.
                    drag(60, 40, 0, 110)
                    assert state()["taps"] == 1
                    drag(100, 200, 0, 80)
                    until("JSON.parse(window.__BOKHEIM_MOBILE_PROBE__).scrolls >= 2")
                    contacts("touchStart", [(100, 200)])
                    until("JSON.parse(window.__BOKHEIM_MOBILE_PROBE__).holds === 1")
                    contacts("touchEnd", [])
                    if session:
                        session.send("Input.synthesizePinchGesture", {"x": 150, "y": 200, "scaleFactor": 1.8, "relativeSpeed": 1000, "gestureSourceType": "touch"})
                    else:
                        contacts("touchStart", [(80, 200), (180, 200)])
                        contacts("touchMove", [(60, 200), (240, 200)])
                        contacts("touchEnd", [])
                    until("JSON.parse(window.__BOKHEIM_MOBILE_PROBE__).pinches >= 3")
                    contacts("touchStart", [(100, 200)])
                    contacts("touchMove", [(100, 280)])
                    contacts("touchCancel", [])
                    page.evaluate("document.querySelector('canvas').addEventListener('pointerdown', event => globalThis.__probePointer = event.pointerId, {once: true, capture: true})")
                    contacts("touchStart", [(60, 40)])
                    page.evaluate("document.querySelector('canvas').dispatchEvent(new PointerEvent('lostpointercapture', {pointerType: 'touch', pointerId: __probePointer}))")
                    contacts("touchEnd", [])
                    assert state()["taps"] == 1
                    contacts("touchStart", [(60, 40)])
                    page.evaluate("Object.defineProperty(document, 'visibilityState', {configurable: true, value: 'hidden'}); document.dispatchEvent(new Event('visibilitychange'))")
                    contacts("touchEnd", [])
                    page.evaluate("delete document.visibilityState; document.dispatchEvent(new Event('visibilitychange'))")
                    assert state()["taps"] == 1
                    # The production scrub listener claims a drag and keeps it
                    # through a release outside the thin track, without scrolling.
                    scrolls = state()["scrolls"]
                    contacts("touchStart", [(40, 300)])
                    until("JSON.parse(window.__BOKHEIM_MOBILE_PROBE__).scrub !== null")
                    contacts("touchMove", [(220, 340)])
                    until("JSON.parse(window.__BOKHEIM_MOBILE_PROBE__).scrub === 220")
                    assert state()["seeks"] == 0, state()
                    contacts("touchEnd", [])
                    until("JSON.parse(window.__BOKHEIM_MOBILE_PROBE__).seeks === 1")
                    assert state()["lastSeek"] > 180, state()
                    assert state()["scrub"] is None, state()
                    assert state()["scrolls"] == scrolls, state()
                    contacts("touchStart", [(40, 300)])
                    until("JSON.parse(window.__BOKHEIM_MOBILE_PROBE__).scrub !== null")
                    contacts("touchMove", [(180, 340)])
                    contacts("touchCancel", [])
                    until("JSON.parse(window.__BOKHEIM_MOBILE_PROBE__).scrub === null")
                    assert state()["seeks"] == 1, state()
                    tap(60, 100)
                    until("document.querySelector('input').getAttribute('inputmode') === 'text'")
                    # Padding belongs to the editable control, including when
                    # the field already has focus or its keyboard was hidden.
                    for x, y in [(60, 82), (18, 100), (294, 100), (60, 110)]:
                        tap(x, y)
                        assert page.locator("input").first.get_attribute("inputmode") == "text", (x, y)
                    tap(60, 420)
                    until("document.querySelector('input').getAttribute('inputmode') === 'none'")
                    tap(60, 82)
                    until("document.querySelector('input').getAttribute('inputmode') === 'text'")
                    page.evaluate("document.querySelector('input').dispatchEvent(new InputEvent('beforeinput', {inputType: 'insertText', data: 'Mobile', bubbles: true, cancelable: true}))")
                    until("JSON.parse(window.__BOKHEIM_MOBILE_PROBE__).text === 'Mobile'")
                    page.evaluate("document.querySelector('input').dispatchEvent(new InputEvent('beforeinput', {inputType: 'deleteContentBackward', bubbles: true, cancelable: true}))")
                    until("JSON.parse(window.__BOKHEIM_MOBILE_PROBE__).text === 'Mobil'")
                    page.evaluate("""() => {
                        const input = document.querySelector('input');
                        input.dispatchEvent(new CompositionEvent('compositionstart', {data: ''}));
                        input.dispatchEvent(new CompositionEvent('compositionupdate', {data: 'é'}));
                        input.dispatchEvent(new CompositionEvent('compositionend', {data: 'é'}));
                    }""")
                    until("JSON.parse(window.__BOKHEIM_MOBILE_PROBE__).text === 'Mobilé'")
                    # Simulate keyboard occlusion without changing the document's
                    # canvas size: the production viewport handler supplies insets.
                    page.evaluate("""() => {
                        const height = innerHeight;
                        globalThis.__probeInnerHeight = Object.getOwnPropertyDescriptor(window, 'innerHeight');
                        Object.defineProperty(window, 'innerHeight', {configurable: true, get: () => height - 240});
                        Object.defineProperty(visualViewport, 'height', {configurable: true, get: () => height - 240});
                        visualViewport.dispatchEvent(new Event('resize'));
                    }""")
                    until("JSON.parse(window.__BOKHEIM_MOBILE_PROBE__).keyboardBottom === 240")
                    assert state()["height"] == initial["height"]
                    page.evaluate("Object.defineProperty(window, 'innerHeight', __probeInnerHeight); delete visualViewport.height; visualViewport.dispatchEvent(new Event('resize'))")
                    until("JSON.parse(window.__BOKHEIM_MOBILE_PROBE__).keyboardBottom === 0")
                    page.evaluate("""() => {
                        const probe = document.querySelector('div[aria-hidden="true"]');
                        globalThis.__probePadding = probe.style.paddingBottom;
                        probe.style.paddingBottom = '17px';
                        visualViewport.dispatchEvent(new Event('resize'));
                    }""")
                    until("JSON.parse(window.__BOKHEIM_MOBILE_PROBE__).safeBottom === 17")
                    page.evaluate("document.querySelector('div[aria-hidden=\"true\"]').style.paddingBottom = __probePadding; visualViewport.dispatchEvent(new Event('resize'))")
                    tap(60, 420)
                    until("document.querySelector('input').getAttribute('inputmode') === 'none'")
                    tap(60, 488)
                    until("document.activeElement?.id === 'probe-dom-input'")
                    page.locator('#probe-dom-input').fill('DOM-only text')
                    assert state()["text"] == 'Mobilé'
                    page.evaluate("document.querySelector('#probe-dom-dialog').close(); document.querySelector('#probe-dom-dialog').remove()")
                    # Input capability changes must refresh cached view metrics.
                    if session:
                        session.send("Emulation.setTouchEmulationEnabled", {"enabled": False})
                        until("JSON.parse(window.__BOKHEIM_MOBILE_PROBE__).controlHeight === 32")
                        session.send("Emulation.setTouchEmulationEnabled", {"enabled": True, "maxTouchPoints": 5})
                        until("JSON.parse(window.__BOKHEIM_MOBILE_PROBE__).controlHeight === 48")
                    page.set_viewport_size({"width": 900, "height": 600})
                    until("JSON.parse(window.__BOKHEIM_MOBILE_PROBE__).width === 900")
                    assert not state()["compact"]
                    assert state()["readerTouch"]
                else:
                    page.mouse.click(60, 40)
                    until("JSON.parse(window.__BOKHEIM_MOBILE_PROBE__).taps === 1")
                    page.mouse.click(60, 100)
                    page.keyboard.type("Desktop")
                    until("JSON.parse(window.__BOKHEIM_MOBILE_PROBE__).text === 'Desktop'")
                assert not errors, errors
                results.append({"case": name, "initial": initial, "final": state()})
                context.close()
            browser.close()
    finally:
        server.shutdown()
        server.server_close()
        thread.join()
    print(json.dumps(results, indent=2))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("bundle", type=Path)
    parser.add_argument("--browser", choices=["chromium", "webkit"], default="chromium")
    parser.add_argument("--wasm", type=Path, help="Package the built mobile_probe WASM into the bundle before testing")
    args = parser.parse_args()
    if args.wasm:
        prepare(args.bundle.resolve(), args.wasm.resolve())
    run(args.bundle.resolve(), args.browser)
