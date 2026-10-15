#!/usr/bin/env python3
"""Validate Kobo package paths and optionally verify installed bytes or write a manifest."""
import argparse
import hashlib
from pathlib import Path, PurePosixPath
import tarfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('archive', type=Path)
    parser.add_argument('--installed', type=Path)
    parser.add_argument('--manifest', type=Path)
    arguments = parser.parse_args()
    manifest = []
    with tarfile.open(arguments.archive) as archive:
        members = archive.getmembers()
        total = 0
        for member in members:
            path = PurePosixPath(member.name)
            if path.is_absolute() or '..' in path.parts or not (member.isfile() or member.isdir()):
                raise RuntimeError(f'Unsafe package entry: {member.name}')
            if '\n' in member.name or '\r' in member.name or '\\' in member.name:
                raise RuntimeError(f'Unsupported package filename: {member.name}')
            if path.parts and path.parts[0] not in ('.adds', 'Bokheim'):
                raise RuntimeError(f'Unexpected package entry: {member.name}')
            if path.is_relative_to('Bokheim/Libraries') and path != PurePosixPath('Bokheim/Libraries'):
                raise RuntimeError(f'Package would overwrite a library: {member.name}')
            bundled_taxonomy = path == PurePosixPath('.adds/bokheim/taxonomy.sqlite3')
            if (not bundled_taxonomy and path.suffix in ('.sqlite3', '.sqlite', '.db', '.json', '.sqlite3-wal', '.sqlite3-shm')) or any(
                path.is_relative_to(f'.adds/bokheim/{directory}')
                for directory in ('data', 'libraries', 'remote-book-cache', 'taxonomy')
            ):
                raise RuntimeError(f'Package would overwrite application data: {member.name}')
            total += member.size
            if total > 512 * 1024 * 1024:
                raise RuntimeError('Kobo package is unexpectedly large')
        if arguments.installed is not None or arguments.manifest is not None:
            for member in members:
                if not member.isfile():
                    continue
                path = PurePosixPath(member.name)
                expected = hashlib.file_digest(archive.extractfile(member), 'sha256').hexdigest()
                if arguments.installed is not None:
                    with (arguments.installed / path).open('rb') as installed:
                        if hashlib.file_digest(installed, 'sha256').hexdigest() != expected:
                            raise RuntimeError(f'Installed file failed verification: {member.name}')
                manifest.append(f'{expected}  {path}\n')
    if arguments.manifest is not None:
        arguments.manifest.write_text(''.join(manifest))


if __name__ == '__main__':
    main()
