#!/usr/bin/env python3
"""Find hosted volume candidates across providers, without authorizing a merge."""
import argparse
from collections import defaultdict
import importlib.util
import json
from pathlib import Path
import re
import sqlite3
import unicodedata
import xml.etree.ElementTree as ET
import zipfile

SCOPE_SPEC = importlib.util.spec_from_file_location('catalog_duplicate_scope', Path(__file__).with_name('deduplicate-catalog.py'))
scope_reader = importlib.util.module_from_spec(SCOPE_SPEC)
SCOPE_SPEC.loader.exec_module(scope_reader)

NUMBERS = {
    'first': 1, 'third': 3, 'fourth': 4, 'fifth': 5, 'sixth': 6, 'seventh': 7,
    'eighth': 8, 'ninth': 9, 'tenth': 10,
    'one': 1, 'two': 2, 'three': 3, 'four': 4, 'five': 5, 'six': 6,
    'seven': 7, 'eight': 8, 'nine': 9, 'ten': 10, 'eleven': 11,
    'twelve': 12, 'thirteen': 13, 'fourteen': 14, 'fifteen': 15,
    'sixteen': 16, 'seventeen': 17, 'eighteen': 18, 'nineteen': 19,
    'twenty': 20, 'premier': 1, 'première': 1, 'deuxième': 2, 'second': 2,
    'seconde': 2, 'troisième': 3, 'quatrième': 4, 'cinquième': 5,
    'sixième': 6, 'septième': 7, 'huitième': 8, 'neuvième': 9, 'dixième': 10,
    'erster': 1, 'erste': 1, 'erstes': 1, 'zweiter': 2, 'zweite': 2, 'zweites': 2,
    'dritter': 3, 'dritte': 3, 'drittes': 3, 'vierter': 4, 'vierte': 4, 'viertes': 4,
    'fünfter': 5, 'fünfte': 5, 'sechster': 6, 'sechste': 6,
    'eerste': 1, 'tweede': 2, 'derde': 3,
    'första': 1, 'andra': 2, 'tredje': 3, 'fjärde': 4, 'fjerde': 4,
    'femte': 5, 'sjätte': 6, 'förra': 1, 'senare': 2,
}
NUMBER = r'(?:\d+|[ivxlcdm]+|' + '|'.join(NUMBERS) + r')'
MARKER = re.compile(
    r'\b(?P<kind>volumes?|vols?|v(?=\.)|tomes?|tomos?|tomi?|band|bände|bind|bd|delar|del|deel|delen|parts?|teil|parte|books?|buch)'
    r'(?:\.?\s+|\.\s*|\s*[,;:\-–—]\s*|(?=\d))(?:the\s+)?(?P<number>' + NUMBER + r')\b', re.I)
PREFIXED_MARKER = re.compile(
    r'\b(?P<number>\d+|[ivxlcdm]+|erster|erste|erstes|zweiter|zweite|zweites|'
    r'dritter|dritte|drittes|vierter|vierte|viertes|fünfter|fünfte|sechster|sechste|'
    r'första|andra|tredje|fjärde|fjerde|femte|sjätte|förra|senare)'
    r'(?:\.\s*|\s+)(?P<kind>band|bände|teil|bd|delen|bandet)\b', re.I)


def number(text):
    text = text.casefold()
    if text.isdecimal():
        return int(text)
    if text in NUMBERS:
        return NUMBERS[text]
    values = {'i': 1, 'v': 5, 'x': 10, 'l': 50, 'c': 100, 'd': 500, 'm': 1000}
    return sum(-values[c] if i + 1 < len(text) and values[c] < values[text[i + 1]]
               else values[c] for i, c in enumerate(text))


def normalized(text):
    return ' '.join(re.findall(r'\w+', unicodedata.normalize('NFKC', text).casefold()))


def parse_title(title, allow_bare_number=False):
    matches = [m for pattern in (MARKER, PREFIXED_MARKER) if (m := pattern.search(title))]
    match = min(matches, key=lambda m: m.start()) if matches else None
    if not match:
        position = re.search(r'\((\d+)\s*(?:/|\s+(?:of|de|von|van|av)\s+)\s*(\d+)\)', title, re.I)
        if (position and title[:position.start()].strip()
                and 1 <= int(position[1]) <= int(position[2])
                and not (int(position[1]) >= 1000 and int(position[2]) >= 1000)):
            return {'base_title': title[:position.start()].rstrip(' .,-—–:/(['),
                    'number': int(position[1]), 'kind': 'volume',
                    'stated_total': int(position[2]), 'scope': 'single_volume'}
        if allow_bare_number:
            compound = re.search(r'(?:\.\s*|\s+)([IVX]{1,5}):([1-9][0-9]?)\.?$', title)
            if compound and title[:compound.start()].strip():
                return {'base_title': title[:compound.start()].rstrip(' .,-—–:/(['),
                        'number': number(compound[1]), 'kind': 'volume',
                        'stated_total': None, 'scope': 'excerpt',
                        'components': [{'kind': 'part', 'number': int(compound[2]), 'stated_total': None}]}
            bare = re.search(r'(?:\.\s*|\s+)([IVX]{1,5}|[1-9]|1[0-9]|20)\.?$', title)
            if bare and title[:bare.start()].strip() and 1 <= number(bare[1]) <= 20:
                return {'base_title': title[:bare.start()].rstrip(' .,-—–:/(['),
                        'number': number(bare[1]), 'kind': 'volume',
                        'stated_total': None, 'scope': 'single_volume',
                        'numbering_evidence': 'bare_terminal_number_requires_membership_review'}
        return None
    if not title[:match.start()].strip():
        return None
    suffix = title[match.end():]
    total = re.match(r'^[\s.,:;]*(?:\(?\s*(?:of|de|von|van|av)\s+|/\s*)(' + NUMBER + r')\b', suffix, re.I)
    repeated = r'(?:(?:volumes?|vols?|v\.)\s+)?' if match['kind'].casefold() in ('volume', 'volumes', 'vol', 'vols', 'v') else ''
    bundled = re.match(r'^\s*(?:[-–—]|and|et|och)\s*' + repeated + '(' + NUMBER + r')\b', suffix, re.I)
    excerpt = re.search(r'\b(?:slice|essay|chapter|part)\s+' + NUMBER + r'\b', suffix, re.I)
    base_title = title[:match.start()].rstrip(' .,-—–:/([')
    # Michelet's numbered history adds each volume's historical coverage to
    # the shared title. The separately edited Moyen Âge series keeps its title.
    if re.fullmatch(r'Histoire de France\s+\d{3,4}\s*[-–—]\s*\d{3,4}', base_title, re.I):
        base_title = 'Histoire de France'
    # Gutenberg prefixes each hearing volume with its position in the larger
    # 26-volume evidence collection. That position is not part of the shared
    # title of the 15-volume hearings; retain it in the original package title.
    if re.fullmatch(r'Warren Commission\s*\(\d+\s+of\s+26\)\s*:\s*Hearings', base_title, re.I):
        base_title = 'Warren Commission: Hearings'
    kind = match['kind'].casefold()
    book_range = re.match(r'^\s*(?:[-–—]|to)\s*(?:(' + NUMBER + r')\b|the\s+end\b)', suffix, re.I) if kind in ('book', 'books', 'buch') else None
    result = {
        'base_title': base_title,
        'number': number(match['number']),
        'kind': 'book' if kind in ('book', 'books', 'buch') else ('part' if kind in ('part', 'parts', 'teil', 'parte') else 'volume'),
        'stated_total': number(total[1]) if total else None,
        'scope': 'book_range' if book_range else ('already_bundled' if bundled else ('excerpt' if excerpt else 'single_volume')),
    }
    if book_range:
        result['range_end'] = number(book_range[1]) if book_range[1] else None
    if excerpt and result['scope'] == 'excerpt':
        components = []
        pattern = r'\b(slice|essay|chapter|part)\s+(' + NUMBER + r')\b(?:\s+(?:of|/)\s*(' + NUMBER + r')\b)?'
        for component in re.finditer(pattern, suffix, re.I):
            components.append({'kind': component[1].casefold(), 'number': number(component[2]),
                               'stated_total': number(component[3]) if component[3] else None})
        result['components'] = components
    return result


def group_component_candidates(entries):
    """Discover files that collectively reconstruct one printed volume."""
    groups = defaultdict(list)
    for entry in entries:
        components = entry.get('components', [])
        if entry['scope'] != 'excerpt' or not components:
            continue
        kinds = {c['kind'] for c in components}
        if len(kinds) != 1:
            continue
        book_creators = tuple(entry['creators']) if entry['kind'] == 'book' else ()
        key = (normalized(entry['base_title']), entry['kind'], entry['number'],
               next(iter(kinds)), tuple(entry['languages']), book_creators)
        groups[key].append(entry)
    results = []
    for key, files in groups.items():
        files.sort(key=lambda v: (min(c['number'] for c in v['components']), v['hash']))
        numbers = sorted(c['number'] for v in files for c in v['components'])
        totals = sorted({c['stated_total'] for v in files for c in v['components'] if c['stated_total']})
        if len(numbers) != len(set(numbers)):
            status = 'duplicate_components_need_review'
        elif len(totals) > 1:
            status = 'conflicting_component_extent_needs_review'
        elif not totals:
            status = 'component_extent_needs_review'
        elif numbers != list(range(1, totals[0] + 1)):
            status = 'missing_components'
        else:
            status = 'contiguous_component_range_needs_edition_review'
        results.append({'title': files[0]['base_title'], 'parent_kind': key[1],
                        'parent_number': key[2], 'component_kind': key[3],
                        'numbers': numbers, 'stated_totals': totals, 'status': status,
                        'credit_variation': len({(tuple(v['creators']), tuple(v['contributors'])) for v in files}) > 1,
                        'volumes': files})
    return sorted(results, key=lambda g: (g['title'], g['parent_number'], g['component_kind']))


def group_candidates(entries):
    groups = defaultdict(list)
    for entry in entries:
        if entry['scope'] in ('single_volume', 'book_range'):
            # Credits can legitimately differ for introductions, illustrations,
            # and translators. They are evidence to review, not discovery keys.
            # Book divisions belong to a particular author's work. Livy and
            # Mommsen both have a History of Rome but are separate histories.
            book_creators = tuple(entry['creators']) if entry['kind'] == 'book' else ()
            groups[(normalized(entry['base_title']), entry['kind'],
                    tuple(entry['languages']), book_creators)].append(entry)
    results = []
    for volumes in groups.values():
        volumes.sort(key=lambda v: (v['number'], v['hash']))
        numbers = [v['number'] for v in volumes]
        totals = sorted({v['stated_total'] for v in volumes if v['stated_total']})
        if len(numbers) != len(set(numbers)):
            status = 'duplicate_numbers_need_review'
        elif any(v['scope'] == 'book_range' for v in volumes):
            status = 'book_ranges_need_extent_review'
        elif len(totals) > 1:
            status = 'conflicting_extent_needs_review'
        elif not totals:
            status = 'extent_needs_review'
        elif numbers != list(range(1, totals[0] + 1)):
            status = 'missing_volumes'
        else:
            status = 'contiguous_stated_range_needs_edition_review'
        results.append({
            'title': volumes[0]['base_title'], 'status': status,
            'numbers': numbers, 'stated_totals': totals,
            'credit_variation': len({(tuple(v['creators']), tuple(v['contributors'])) for v in volumes}) > 1,
            'volumes': volumes,
        })
    return sorted(results, key=lambda g: (g['status'], g['title']))


def survey(catalog):
    entries, exclusions = [], []
    with sqlite3.connect(f'file:{catalog / "curation.sqlite3"}?mode=ro', uri=True) as db:
        sources = defaultdict(set)
        for book_hash, source in db.execute('SELECT book_hash,source FROM source_entry WHERE book_hash IS NOT NULL'):
            sources[book_hash].add(source)
        for book_hash, title in db.execute('SELECT hash,title FROM book'):
            try:
                with zipfile.ZipFile(catalog / 'books' / (book_hash + '.epub')) as archive:
                    container = ET.fromstring(archive.read('META-INF/container.xml'))
                    rootfile = container.find('.//{*}rootfile')
                    package = ET.fromstring(archive.read(rootfile.get('full-path')))
                    metadata = package.find('{*}metadata')
                    package_title = metadata.findtext('{*}title') or title
                    swedish = metadata.findtext('{*}language', '') in ('sv', 'swe', 'sv-SE')
                    parsed = (parse_title(package_title, allow_bare_number=swedish)
                              or parse_title(title, allow_bare_number=swedish))
                    titlepage_markers = []
                    if parsed is None:
                        titlepage_markers = scope_reader.titlepage_volume_markers(archive, rootfile.get('full-path'), package)
                        if len(titlepage_markers) == 1:
                            parsed = parse_title(package_title + ', ' + titlepage_markers[0])
                        if parsed is None:
                            continue
                    if package.get('version') != '2.0' and not package.get('version', '').startswith('3.'):
                        exclusions.append({'hash': book_hash, 'title': title, 'reason': 'unsupported_epub_version'})
                        continue
                    entry = {'hash': book_hash, 'title': title, 'package_title': package_title,
                             'epub_version': package.get('version'),
                             'sources': sorted(sources[book_hash]), **parsed}
                    if titlepage_markers:
                        entry['original_titlepage_volume_marker'] = titlepage_markers[0]
                    for key, tag in [('creators', 'creator'), ('contributors', 'contributor'), ('languages', 'language')]:
                        entry[key] = sorted(' '.join(e.itertext()).strip() for e in metadata.findall('{*}' + tag))
                    entries.append(entry)
            except (OSError, ValueError, AttributeError, ET.ParseError, zipfile.BadZipFile, KeyError) as error:
                exclusions.append({'hash': book_hash, 'title': title, 'reason': 'inspection_error', 'detail': str(error)})
    return {'groups': group_candidates(entries),
            'component_groups': group_component_candidates(entries),
            'separate_scopes': [v for v in entries if v['scope'] not in ('single_volume', 'book_range')],
            'excluded': exclusions,
            'policy': 'Candidates only: verify complete work, source contents and edition before merging. External audiobooks are excluded.'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--catalog', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    report = survey(args.catalog)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
    print(f"{len(report['groups'])} candidate groups; {len(report['excluded'])} excluded inputs")


if __name__ == '__main__':
    main()
