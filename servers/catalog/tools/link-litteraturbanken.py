#!/usr/bin/env python3
"""Prepare source-page entries from an existing survey, without fetching files.

Usage: link-litteraturbanken.py SURVEY_DIRECTORY MANIFEST_JSON
Only non-CC0 containing editions are included; generated parts are folded into
those editions. Rights identifiers are retained, but copyrighted introductions
and cover images are not copied. Missing source languages remain unknown.
"""
import argparse
from collections import Counter
import importlib.util
import json
from pathlib import Path
from urllib.parse import quote
import uuid


def prepare(survey):
    spec = importlib.util.spec_from_file_location('survey', Path(__file__).with_name('survey-litteraturbanken.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    rows = json.loads((survey / 'epub-list.json').read_text())['data']
    groups = {}
    for row in rows:
        groups.setdefault(module.epub_url(row), []).append(row)
    languages = {'swe': 'sv', 'eng': 'en', 'fra': 'fr', 'deu': 'de', 'smi': 'smi'}
    roles = {'translator': 'trl', 'editor': 'edt', 'illustrator': 'ill', 'compiler': 'com'}
    formats = {'roman': 'FIC000000', 'novell': 'FIC029000', 'novellsamling': 'FIC029000', 'diktsamling': 'POE000000', 'dikt': 'POE000000', 'drama': 'DRA000000', 'dramasamling': 'DRA000000'}
    entries = []
    for file_url, group in groups.items():
        if all(row['epub_license'] == 'cc-0' for row in group):
            continue
        row = next((row for row in group if not row.get('generated_from_part')), None)
        if row is None:
            raise ValueError('No containing edition for ' + repr([row['titlepath'] for row in group]))
        licenses = sorted({row['epub_license'] for row in group})
        if len(licenses) != 1:
            raise ValueError('Mixed licenses need review: ' + row['title'])
        authorid = row['main_author']['authorid']
        url = 'https://litteraturbanken.se/författare/' + quote(authorid) + '/titlar/' + quote(row['titlepath'], safe='/') + '/info'
        contributors = []
        for author in row.get('authors') or [row['main_author']]:
            role = roles.get(author.get('type'), 'aut')
            contributors.append({'contributor_id': str(uuid.uuid5(uuid.NAMESPACE_URL, 'https://litteraturbanken.se/författare/' + author['authorid'])), 'name': author['full_name'], 'role': list(role.encode('ascii'))})
        subjects = []
        texttype = row.get('texttype')
        if texttype:
            subjects.append({'id': None, 'name': texttype, 'sort_name': None, 'source': 'catalog:litteraturbanken.se:texttype', 'authority': None, 'code': None})
        if texttype in formats:
            code = formats[texttype]
            subjects.append({'id': None, 'name': code, 'sort_name': None, 'source': 'catalog:litteraturbanken.se:texttype:reviewed', 'authority': 'bisac', 'code': code})
        identifiers = [{'value': row['lbworkid'], 'scheme': {'other': 'litteraturbanken'}, 'scope': 'edition'}]
        if row.get('librisid'):
            identifiers.append({'value': row['librisid'], 'scheme': {'other': 'libris'}, 'scope': 'edition'})
        language = languages.get(row.get('language'))
        # Litteraturbanken's language filter groups Sami varieties as "smi".
        # Its own exhibition identifies this translation as Northern Sami:
        # https://litteraturbanken.se/diktensmuseum/samisk-litteratur-texter-pa-samiska-sprak/
        if row['lbworkid'] == 'lb13688066':
            language = 'se'
        record = {'title': ' '.join(row['title'].split()), 'subtitle': None, 'contributors': contributors, 'description': '', 'book': {'identifiers': identifiers, 'publishers': row.get('publisher', [])[:1], 'languages': [language] if language else [], 'year': int(row['imprintyear']), 'subjects': subjects}}
        entries.append({'source': 'litteraturbanken.se', 'entry_id': row['lbworkid'], 'entry_url': url, 'file_url': file_url, 'license': licenses[0], 'format': 'Epub', 'record': record})
    return sorted(entries, key=lambda entry: entry['entry_id'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('survey', type=Path)
    parser.add_argument('manifest', type=Path)
    args = parser.parse_args()
    entries = prepare(args.survey)
    args.manifest.parent.mkdir(parents=True, exist_ok=True)
    args.manifest.write_text(json.dumps(entries, ensure_ascii=False, indent=2) + '\n')
    print(json.dumps({'entries': len(entries), 'licenses': dict(Counter(entry['license'] for entry in entries)), 'languages': dict(Counter(language for entry in entries for language in entry['record']['book']['languages'])), 'unknown_language': sum(not entry['record']['book']['languages'] for entry in entries)}, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    main()
