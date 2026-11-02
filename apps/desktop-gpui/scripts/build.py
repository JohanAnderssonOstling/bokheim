#!/usr/bin/env python3
"""Build the desktop app, its verification browser, and PDFium runtime."""
import argparse
import os
from pathlib import Path
import runpy
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[3]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target")
    parser.add_argument("--flatpak", action="store_true")
    parser.add_argument("--offline", action="store_true")
    parser.add_argument("--without-browser", action="store_true",
                        help="Build the Linux app without the optional WebKitGTK browser helper")
    parser.add_argument("--speech-from-source", action="store_true",
                        help="Build reduced static speech libraries for x86-64 Linux")
    parser.add_argument("--speech-work", type=Path)
    parser.add_argument("--speech-jobs", type=int, default=1)
    parser.add_argument("--diagnostics", action="store_true",
                        help="Retain debug and trace logging in the release executable")
    args = parser.parse_args()
    requested_target = args.target or os.environ.get("CARGO_BUILD_TARGET")
    target = requested_target or next(line.removeprefix("host: ") for line in
        subprocess.check_output(["rustc", "-vV"], text=True).splitlines() if line.startswith("host: "))
    if args.without_browser and "-linux-" not in target:
        parser.error("--without-browser is supported only for Linux packages")
    if args.speech_from_source and target != "x86_64-unknown-linux-gnu":
        parser.error("--speech-from-source currently requires x86_64-unknown-linux-gnu")
    prepare = runpy.run_path(str(ROOT / "shared/pdfium/prepare.py"))["prepare"]
    prepared = prepare(target, offline=args.offline)
    environment = dict(os.environ)
    if target == "x86_64-unknown-linux-gnu":
        linux_environment = runpy.run_path(str(ROOT / "scripts/linux/toolchain.py"))["build_environment"]
        environment.update(linux_environment(offline=args.offline))
    environment["PDFIUM_PREPARED_DIR"] = str(prepared)
    native_speech_libraries = None
    if args.speech_from_source:
        native_speech = runpy.run_path(str(ROOT / "shared/reader-speech/build_native.py"))["prepare_native"]
        native_speech_libraries = native_speech(work=args.speech_work, jobs=args.speech_jobs, offline=args.offline)
        environment["SHERPA_ONNX_LIB_DIR"] = str(native_speech_libraries)
    features = ["flatpak" if args.flatpak else "desktop"]
    if not args.diagnostics:
        features.append("release-logging")
    command = ["cargo", "build", "--release", "-p", "desktop-gpui", "--bin", "bokheim",
               "--no-default-features", "--features", ",".join(features)]
    if requested_target:
        command += ["--target", target]
    if args.offline:
        command += ["--offline"]
    subprocess.run(command, cwd=ROOT, check=True, env=environment)
    helper = ["cargo", "build", "--release", "-p", "opds-webview"]
    if not args.diagnostics:
        helper += ["--features", "release-logging"]
    if requested_target:
        helper += ["--target", target]
    if args.offline:
        helper += ["--offline"]
    if not args.without_browser:
        if target == "x86_64-unknown-linux-gnu":
            browser_environment = runpy.run_path(str(ROOT / "scripts/linux/browser-toolchain.py"))["build_environment"]
            # System WebKit/GLib resolve their own libc references at runtime.
            # Apply this only to the helper; its direct glibc imports are audited below.
            helper[1] = "rustc"
            helper += ["--", "-C", "link-arg=-Wl,--allow-shlib-undefined"]
            subprocess.run(helper, cwd=ROOT, check=True, env=browser_environment(offline=args.offline))
        else:
            subprocess.run(helper, cwd=ROOT, check=True)
    output = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target"))
    if not output.is_absolute():
        output = ROOT / output
    if requested_target:
        output /= target
    if target == "x86_64-unknown-linux-gnu":
        subprocess.run(["bash", str(ROOT / "apps/desktop-gpui/linux/appimage/verify-glibc-compatibility.sh"),
                        str(output / "release/bokheim"), "2.17"], check=True)
        if not args.without_browser:
            subprocess.run(["bash", str(ROOT / "apps/desktop-gpui/linux/appimage/verify-glibc-compatibility.sh"),
                            str(output / "release/bokheim-opds-webview"), "2.17"], check=True)
    prepare_speech = runpy.run_path(str(ROOT / "shared/reader-speech/prepare.py"))
    if native_speech_libraries is not None:
        prepare_speech["prepare_ort"](output / "release/tts", native_speech_libraries.parent, offline=args.offline)
        shutil.copy2(native_speech_libraries.parent / "LICENSE", output / "release/tts/RUNTIME-LICENSE.txt")
        shutil.copy2(native_speech_libraries.parent / "receipt.json", output / "release/tts/RUNTIME-SOURCE.json")
    else:
        prepare_speech["prepare_all"](output / "release/tts", offline=args.offline)
        for name in ("RUNTIME-LICENSE.txt", "RUNTIME-SOURCE.json"):
            (output / "release/tts" / name).unlink(missing_ok=True)
    if target != "x86_64-pc-windows-msvc":
        prepare(target, destination=output / "release/pdfium", offline=True)


if __name__ == "__main__":
    main()
