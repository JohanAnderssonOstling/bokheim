#!/usr/bin/env python3
"""Persistent Cargo targets shared by Bokheim checkouts on this machine."""
import argparse
import os
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[2]


def target_directory(platform, root=ROOT, environ=None):
    environ = os.environ if environ is None else environ
    if not re.fullmatch(r'[a-zA-Z0-9_-]+', platform):
        raise ValueError(f'Invalid build cache platform: {platform}')
    configured = environ.get('CARGO_TARGET_DIR')
    if configured:
        path = Path(configured).expanduser()
        return (root / path).resolve()
    # Hosted runners restore this directory with the platform's Actions cache.
    if environ.get('GITHUB_ACTIONS') == 'true':
        return root / 'target'
    cache = environ.get('BOKHEIM_BUILD_CACHE')
    if cache:
        base = Path(cache).expanduser()
    else:
        user_cache = environ.get('XDG_CACHE_HOME') or environ.get('LOCALAPPDATA')
        base = (Path(user_cache) if user_cache else Path.home() / '.cache') / 'bokheim' / 'builds'
    return (base / platform).resolve()


def environment(platform, root=ROOT, environ=None):
    result = dict(os.environ if environ is None else environ)
    result['CARGO_TARGET_DIR'] = str(target_directory(platform, root, result))
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['path', 'run'])
    parser.add_argument('platform')
    parser.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if args.action == 'path':
        if args.command:
            parser.error('path does not accept a command')
        print(target_directory(args.platform))
    else:
        command = args.command[1:] if args.command[:1] == ['--'] else args.command
        if not command:
            parser.error('run requires a command')
        subprocess.run(command, cwd=ROOT, env=environment(args.platform), check=True)


if __name__ == '__main__':
    main()
