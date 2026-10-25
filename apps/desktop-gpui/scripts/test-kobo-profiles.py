#!/usr/bin/env python3
"""Render the real Kobo application for each declared profile on the host or ARM QEMU.

This checks application layout and capability selection, not physical drivers,
e-ink waveforms, suspend reliability, device performance or battery consumption.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[3]


def inspect_frame(path, screen_width, screen_height):
    with path.open('rb') as image:
        if image.readline().strip() != b'P5':
            raise RuntimeError(f'Invalid grayscale frame: {path}')
        width, height = map(int, image.readline().split())
        if image.readline().strip() != b'255':
            raise RuntimeError(f'Invalid frame depth: {path}')
        pixels = image.read()
    if len(pixels) != width * height or width == 0 or height == 0:
        raise RuntimeError(f'Incomplete frame: {path}')
    if (width, height) != (screen_width, screen_height):
        raise RuntimeError(f'The simulated frame has the wrong device dimensions: {path}')
    if len(set(pixels)) < 2:
        raise RuntimeError(f'The application produced an empty frame: {path}')
    return {'width': width, 'height': height, 'sha256': hashlib.sha256(pixels).hexdigest()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=ROOT / 'target/release/desktop-gpui-kobo')
    parser.add_argument('--output', type=Path, default=ROOT / 'target/kobo-profile-verification')
    parser.add_argument('--models', type=int, nargs='+', help='Limit verification to these numeric profile IDs')
    parser.add_argument('--qemu', type=Path, help='Run an ARM executable with qemu-arm or qemu-arm-static')
    parser.add_argument('--qemu-root', type=Path, help='QEMU prefix containing the executable\'s private interpreter path')
    arguments = parser.parse_args()
    binary = arguments.binary.resolve()
    output = arguments.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    report_path = output / 'report.json'
    report_path.unlink(missing_ok=True)
    command = [str(binary)]
    if arguments.qemu:
        if not arguments.qemu_root:
            parser.error('--qemu requires --qemu-root')
        command = [str(arguments.qemu.resolve()), '-cpu', 'cortex-a8', '-L', str(arguments.qemu_root.resolve()), *command]
    elif arguments.qemu_root:
        parser.error('--qemu-root requires --qemu')
    catalogue = subprocess.check_output([*command, '--list-device-models'], text=True, timeout=90)
    profiles = []
    known = set()
    for line in catalogue.splitlines():
        identifier, product, dimensions, name = line.split('\t')
        identifier = int(identifier)
        width, height = map(int, dimensions.split('x'))
        if identifier in known or identifier <= 0 or width <= 0 or height <= 0:
            raise RuntimeError(f'Invalid or duplicate device profile: {line}')
        known.add(identifier)
        profiles.append((identifier, product, width, height, name))
    if not profiles:
        raise RuntimeError('The application returned an empty device catalogue')
    if arguments.models and set(arguments.models) - known:
        parser.error(f'Unknown device IDs: {sorted(set(arguments.models) - known)}')
    with binary.open('rb') as executable:
        binary_hash = hashlib.file_digest(executable, 'sha256').hexdigest()
    kind = 'ARM user-mode QEMU; host kernel; hardware unverified' if arguments.qemu else 'host simulation; hardware unverified'
    report = {'kind': kind,
              'binary': str(binary), 'binary_sha256': binary_hash, 'profiles': []}
    for identifier, product, width, height, name in profiles:
        if arguments.models and identifier not in arguments.models:
            continue
        directory = output / str(identifier)
        directory.mkdir(exist_ok=True)
        capture = directory / 'application.pgm'
        capture.unlink(missing_ok=True)
        environment = dict(os.environ)
        environment['BOKHEIM_KOBO_INSTALL_ROOT'] = str(directory / 'installation')
        environment['BOKHEIM_KOBO_DATA_DIR'] = str(directory / 'data')
        environment['BOKHEIM_KOBO_LIBRARIES_DIR'] = str(directory / 'libraries')
        environment['BOKHEIM_KOBO_LIBRARY_DIR'] = str(directory / 'libraries/default')
        with (directory / 'application.log').open('w') as log:
            subprocess.run([*command, '--no-display', '--device-model', str(identifier), '--capture', str(capture)],
                           env=environment, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, check=True, timeout=90)
        frame = inspect_frame(capture, width, height)
        report['profiles'].append({'id': identifier, 'product': product, 'name': name, **frame})
        print(f'PASS {identifier} {name}: {frame["width"]}x{frame["height"]}', flush=True)
    report_path.write_text(json.dumps(report, indent=2) + '\n')


if __name__ == '__main__':
    main()
