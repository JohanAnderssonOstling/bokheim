#!/usr/bin/env python3
"""Install only the validated local catalog release, retaining the old snapshot."""
import hashlib
import json
from pathlib import Path
import shutil
import sqlite3
from datetime import datetime, timezone

ROOT = Path(__file__).resolve().parent


def readonly(path):
    return sqlite3.connect(path.resolve().as_uri() + '?mode=ro', uri=True)


def digest(path):
    with path.open('rb') as file:
        return hashlib.file_digest(file, 'sha256').hexdigest()


def curation_digest(path):
    result = hashlib.sha256()
    with readonly(path) as db:
        for table, key in [('book', 'hash'), ('external_book', 'entry_key'),
                           ('book_source', 'hash,source'), ('source_entry', 'source,entry_id')]:
            for row in db.execute(f'SELECT * FROM {table} ORDER BY {key}'):
                result.update(json.dumps(row, ensure_ascii=False, separators=(',', ':')).encode())
                result.update(b'\n')
    return result.hexdigest()


def main():
    plan = json.loads((ROOT / 'publication-plan.json').read_text())
    validation = json.loads((ROOT / 'publication-validation.json').read_text())
    manifest = json.loads((ROOT / 'catalog-entry.json').read_text())
    release = plan['release_id']
    staged = Path(plan['output']) / 'releases' / str(release)
    installed = Path(plan['installed_catalog'])
    published = Path(plan['published_root'])
    target = published / 'releases' / str(release)
    assert validation['release_id'] == manifest['release_id'] == release
    assert validation['became_unassigned'] == 0
    assert validation['non_classification_table_differences'] == {}
    assert digest(staged / 'catalog.sqlite3') == validation['snapshot_sha256'] == manifest['artifact']['sha256']
    assert curation_digest(Path(plan['source_catalog']) / 'curation.sqlite3') == plan['curation_content_sha256'], 'Curation changed; rebuild before installing'
    with readonly(installed) as db:
        assert db.execute('SELECT release_id FROM snapshot').fetchone()[0] == plan['previous_release_id'], 'Installed release changed'
        assert db.execute('PRAGMA journal_mode').fetchone()[0] == 'delete', 'Live WAL installation needs a different procedure'
    assert max(int(path.name) for path in (published / 'releases').iterdir() if path.name.isdigit()) + 1 == release
    assert not target.exists(), 'Immutable releases must not be overwritten'

    backup_directory = Path(plan['source_catalog']) / 'audits' / 'literature-code-coverage-20261015'
    backup_directory.mkdir(parents=True, exist_ok=True)
    backup = backup_directory / ('installed-before-' + str(plan['previous_release_id']) + '.sqlite3')
    assert not backup.exists(), 'Keep the original backup'
    with readonly(installed) as source, sqlite3.connect(backup) as destination:
        source.backup(destination)

    target.mkdir()
    # Write the SQLite artifact last, so incomplete sidecars cannot expose an
    # apparently complete release. Existing book/cover assets are already present.
    for name in ['catalog.sqlite3.gz', 'catalog.sqlite3.zst', 'catalog.sqlite3']:
        partial = target / (name + '.partial')
        shutil.copyfile(staged / name, partial)
        assert digest(partial) == digest(staged / name)
        partial.replace(target / name)
    (backup_directory / 'catalog-entry.json').write_text(json.dumps(manifest, indent=2) + '\n')
    partial = installed.with_name('catalog.sqlite3.literature-248.partial')
    assert not partial.exists()
    shutil.copyfile(target / 'catalog.sqlite3', partial)
    assert digest(partial) == validation['snapshot_sha256']
    partial.replace(installed)
    with readonly(installed) as db:
        assert db.execute('SELECT release_id FROM snapshot').fetchone()[0] == release
        assert db.execute('PRAGMA integrity_check').fetchone()[0] == 'ok'
        assert not db.execute('PRAGMA foreign_key_check').fetchall()
    result = dict(release_id=release, installed=str(installed), published=str(target),
                  backup=str(backup), installed_at=datetime.now(timezone.utc).isoformat(),
                  snapshot_sha256=validation['snapshot_sha256'])
    (ROOT / 'installation.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
