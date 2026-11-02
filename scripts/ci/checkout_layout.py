#!/usr/bin/env python3
"""Provide the application directory expected by sibling source dependencies."""
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]


def prepare_checkout():
    alias = ROOT.parent / 'Html-Prototype'
    if alias.exists():
        if alias.resolve() != ROOT:
            raise RuntimeError(f'{alias} belongs to a different application checkout')
        return
    if alias.is_symlink():
        raise RuntimeError(f'Refusing to replace an existing checkout link: {alias}')
    if os.name == 'nt':
        subprocess.run(['cmd', '/c', 'mklink', '/J', str(alias), str(ROOT)], check=True)
    else:
        alias.symlink_to(ROOT, target_is_directory=True)


if __name__ == '__main__':
    prepare_checkout()
