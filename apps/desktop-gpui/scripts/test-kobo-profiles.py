#!/usr/bin/env python3
"""Render the real Kobo application for each declared profile on the host.

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
    if abs(width * screen_height - height * screen_width) > max(screen_width, screen_height):
        raise RuntimeError(f'The simulated frame has the wrong device aspect ratio: {path}')
    if len(set(pixels)) < 2:
        raise RuntimeError(f'The application produced an empty frame: {path}')
    return {'width': width, 'height': height, 'sha256': hashlib.sha256(pixels).hexdigest()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=ROOT / 'target/release/desktop-gpui-kobo')
    parser.add_argument('--output', type=Path, default=ROOT / 'target/kobo-profile-verification')
    parser.add_argument('--models', type=int, nargs='+', help='Limit verification to these numeric profile IDs')
    arguments = parser.parse_args()
    binary = arguments.binary.resolve()
    output = arguments.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    catalogue = subprocess.check_output([str(binary), '--list-device-models'], text=True)
    report = {'kind': 'host simulation; hardware unverified', 'profiles': []}
    known = set()
    for line in catalogue.splitlines():
        identifier, product, dimensions, name = line.split('\t')
        identifier = int(identifier)
        known.add(identifier)
        if arguments.models and identifier not in arguments.models:
            continue
        width, height = map(int, dimensions.split('x'))
        directory = output / str(identifier)
        directory.mkdir(exist_ok=True)
        capture = directory / 'application.pgm'
        environment = dict(os.environ)
        environment['BOKHEIM_KOBO_DATA_DIR'] = str(directory / 'data')
        environment['BOKHEIM_KOBO_LIBRARIES_DIR'] = str(directory / 'libraries')
        environment['BOKHEIM_KOBO_LIBRARY_DIR'] = str(directory / 'libraries/default')
        with (directory / 'application.log').open('w') as log:
            subprocess.run([str(binary), '--no-display', '--device-model', str(identifier), '--capture', str(capture)],
                           env=environment, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, check=True, timeout=90)
        frame = inspect_frame(capture, width, height)
        report['profiles'].append({'id': identifier, 'product': product, 'name': name, **frame})
        print(f'PASS {identifier} {name}: {frame["width"]}x{frame["height"]}', flush=True)
    if arguments.models and set(arguments.models) - known:
        parser.error(f'Unknown device IDs: {sorted(set(arguments.models) - known)}')
    (output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')


if __name__ == '__main__':
    main()
