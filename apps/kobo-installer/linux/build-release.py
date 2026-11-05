#!/usr/bin/env python3
"""Build the Linux Kobo USB installer locally, without running tests."""
import argparse
import json
import os
from pathlib import Path
import platform
import runpy
import subprocess
import sys
import time
import tomllib

ROOT = Path(__file__).resolve().parents[3]
TARGET = 'x86_64-unknown-linux-gnu'


def wait_for_rust():
    announced = False
    while True:
        processes = subprocess.check_output(['ps', '-eo', 'comm='], text=True).splitlines()
        if not any(name.strip() in ('cargo', 'rustc') for name in processes):
            return
        if not announced:
            print('Waiting for the current Rust compilation to finish.', flush=True)
            announced = True
        time.sleep(5)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--archive', type=Path,
                        help='Kobo device archive to embed; defaults to the committed package')
    parser.add_argument('--output', type=Path, default=ROOT / 'release')
    parser.add_argument('--dry-run', action='store_true')
    args = parser.parse_args()
    if platform.system() != 'Linux' or platform.machine() not in ('x86_64', 'AMD64'):
        parser.error('The installer release requires an x86-64 Linux host')
    archive = args.archive or Path(os.environ.get(
        'BOKHEIM_KOBO_ARCHIVE', ROOT / 'apps/kobo-installer/package/Bokheim-Kobo-armv7.tar.gz'))
    archive = archive.expanduser().resolve()
    if not archive.is_file() or not archive.stat().st_size:
        parser.error(f'Missing Kobo device archive: {archive}')
    cache = runpy.run_path(str(ROOT / 'scripts/ci/build_cache.py'))
    env = cache['environment']('linux-glibc217', root=ROOT)
    binary_directory = Path(env['CARGO_TARGET_DIR'])
    if env.get('CARGO_BUILD_TARGET'):
        if env['CARGO_BUILD_TARGET'] != TARGET:
            parser.error(f'CARGO_BUILD_TARGET must be unset or {TARGET}')
        binary_directory /= TARGET
    env.setdefault('CARGO_BUILD_JOBS', '1')
    env.setdefault('RUSTC_BOOTSTRAP', '1')
    with (ROOT / 'apps/desktop-gpui/Cargo.toml').open('rb') as source:
        version = tomllib.load(source)['package']['version']
    env.update(BOKHEIM_KOBO_ARCHIVE=str(archive), BOKHEIM_VERSION=version,
               RELEASE_DIRECTORY=str(args.output.expanduser().resolve()),
               INSTALLER_BINARY=str(binary_directory / 'release/bokheim-kobo-installer'))
    env.setdefault('APPIMAGE_EXTRACT_AND_RUN', '1')
    commands = [
        [sys.executable, str(ROOT / 'scripts/linux/toolchain.py'), '--', 'cargo', 'build',
         '--release', '--locked',
         '--manifest-path', str(ROOT / 'apps/kobo-installer/Cargo.toml')],
        ['bash', str(ROOT / 'apps/kobo-installer/linux/build-appimage.sh')],
    ]
    if args.dry_run:
        print(json.dumps({'commands': commands, 'cache': env['CARGO_TARGET_DIR'],
                          'archive': str(archive), 'version': version,
                          'output': env['RELEASE_DIRECTORY'], 'tests_run': False}, indent=2))
        return
    wait_for_rust()
    for command in commands:
        subprocess.run(command, cwd=ROOT, env=env, check=True)


if __name__ == '__main__':
    main()
