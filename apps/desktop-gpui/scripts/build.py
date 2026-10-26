#!/usr/bin/env python3
"""Build the desktop app, its verification browser, and PDFium runtime."""
import argparse
import os
from pathlib import Path
import runpy
import subprocess

ROOT = Path(__file__).resolve().parents[3]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target")
    parser.add_argument("--flatpak", action="store_true")
    parser.add_argument("--offline", action="store_true")
    parser.add_argument("--without-browser", action="store_true",
                        help="Build the Linux app without the optional WebKitGTK browser helper")
    args = parser.parse_args()
    requested_target = args.target or os.environ.get("CARGO_BUILD_TARGET")
    target = requested_target or next(line.removeprefix("host: ") for line in
        subprocess.check_output(["rustc", "-vV"], text=True).splitlines() if line.startswith("host: "))
    if args.without_browser and "-linux-" not in target:
        parser.error("--without-browser is supported only for Linux packages")
    prepare = runpy.run_path(str(ROOT / "shared/pdfium/prepare.py"))["prepare"]
    prepared = prepare(target, offline=args.offline)
    environment = dict(os.environ)
    if target == "x86_64-unknown-linux-gnu":
        linux_environment = runpy.run_path(str(ROOT / "scripts/linux/toolchain.py"))["build_environment"]
        environment.update(linux_environment(offline=args.offline))
    environment["PDFIUM_PREPARED_DIR"] = str(prepared)
    command = ["cargo", "build", "--release", "-p", "desktop-gpui", "--bin", "bokheim",
               "--no-default-features", "--features", "flatpak" if args.flatpak else "desktop"]
    if requested_target:
        command += ["--target", target]
    if args.offline:
        command += ["--offline"]
    subprocess.run(command, cwd=ROOT, check=True, env=environment)
    helper = ["cargo", "build", "--release", "-p", "opds-webview"]
    if requested_target:
        helper += ["--target", target]
    if args.offline:
        helper += ["--offline"]
    if not args.without_browser:
        subprocess.run(helper, cwd=ROOT, check=True)
    output = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target"))
    if not output.is_absolute():
        output = ROOT / output
    if requested_target:
        output /= target
    if target == "x86_64-unknown-linux-gnu":
        subprocess.run(["bash", str(ROOT / "apps/desktop-gpui/linux/appimage/verify-glibc-compatibility.sh"),
                        str(output / "release/bokheim"), "2.17"], check=True)
    prepare_speech = runpy.run_path(str(ROOT / "shared/reader-speech/prepare.py"))["prepare_all"]
    prepare_speech(output / "release/tts", offline=args.offline)
    if target != "x86_64-pc-windows-msvc":
        prepare(target, destination=output / "release/pdfium", offline=True)


if __name__ == "__main__":
    main()
