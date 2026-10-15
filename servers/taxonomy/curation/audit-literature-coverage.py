#!/usr/bin/env python3
"""Survey LCC coverage without editing the catalog or taxonomy.

Uses the production Rust matcher on an explicit frozen taxonomy. Catalog counts
refer to entries (formats and editions remain separate), not unique works.
Reference probes are active 2025 PQ/PR/PS/PT entries, reviewed mixed-language
literary scopes, and every stored selector/range in the Literature subtree.
Printed spans stored as exact selectors are audited separately from intervals.
"""

import argparse
from collections import Counter, defaultdict
import csv
from functools import lru_cache
import hashlib
import io
import json
from pathlib import Path
import re
import sqlite3
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[3]
DATA = ROOT / 'shared/subject-projection/data'
LITERATURE = 3846
CORE = {'PQ', 'PR', 'PS', 'PT'}
CALL = re.compile(r'^([A-Z]{1,4})\s*(\d+(?:\.\d+)?)(.*)$')


def readonly(path):
    return sqlite3.connect(path.resolve().as_uri() + '?mode=ro', uri=True)


def write_csv(path, rows, fields):
    with path.open('w', newline='') as file:
        writer = csv.DictWriter(file, fieldnames=fields)
        writer.writeheader()
        writer.writerows(rows)


def fingerprint(path):
    with path.open('rb') as file:
        return hashlib.file_digest(file, 'sha256').hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--taxonomy', type=Path, default=DATA / 'unified-taxonomy-v2.sqlite3')
    parser.add_argument('--catalog', type=Path, default=Path.home() / '.local/share/bokheim/catalog/curation.sqlite3')
    parser.add_argument('--installed', type=Path, default=Path.home() / '.local/share/bokheim/catalog.sqlite3')
    parser.add_argument('--reference', type=Path, default=DATA / 'lcc-mdsconnect-2016.sqlite3')
    parser.add_argument('--binary', type=Path, default=ROOT / 'target/release/literature_coverage')
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='.literature-coverage-', dir=args.output) as temporary:
        frozen = {}
        # Freeze mutable inputs through SQLite, including committed WAL data.
        # The large reference stays under a read transaction to avoid copying
        # hundreds of megabytes for an otherwise read-only survey.
        for name in ('taxonomy', 'catalog', 'installed'):
            frozen[name] = Path(temporary) / (name + '.sqlite3')
            with readonly(getattr(args, name)) as source, sqlite3.connect(frozen[name]) as target:
                source.backup(target)
        frozen['reference'] = args.reference
        with readonly(args.reference) as reference:
            reference.execute('BEGIN')
            reference.execute('SELECT count(*) FROM metadata').fetchone()
            audit(args, frozen, reference)


def audit(args, frozen, reference):
    tax = readonly(frozen['taxonomy'])
    labels = dict(tax.execute('SELECT concept_id,preferred_label FROM concept'))
    parents = defaultdict(list)
    for child, parent in tax.execute('SELECT concept_id,parent_concept_id FROM concept_parent ORDER BY concept_id,ordinal'):
        parents[child].append(parent)

    @lru_cache(None)
    def ancestors(identity):
        return {identity}.union(*(ancestors(parent) for parent in parents[identity]))

    @lru_cache(None)
    def paths(identity):
        if not parents[identity]:
            return (labels[identity],)
        return tuple(sorted({prefix + ' / ' + labels[identity]
                             for parent in parents[identity] for prefix in paths(parent)}))

    literature = {identity for identity in labels if LITERATURE in ancestors(identity)}
    root_children = {identity for identity in literature if LITERATURE in parents[identity]}
    # These destinations convey no period, author, genre, or literary topic.
    broad = {LITERATURE} | root_children
    for identity in literature:
        if labels[identity] in {'English', 'American', 'French', 'German', 'Italian',
                                'Spanish', 'Portuguese', 'Scandinavian', 'Dutch',
                                'Literature', 'English Literature', 'American Literature'}:
            broad.add(identity)

    probes = set()
    catalog_codes = defaultdict(set)
    books = {}
    cat = readonly(frozen['catalog'])
    for table, key, fmt in [('book', 'hash', '1'), ('external_book', 'entry_key', 'format')]:
        for identity, title, author, raw, book_format in cat.execute(f'SELECT {key},title,author,record,{fmt} FROM {table}'):
            subjects = json.loads(raw)['book']['subjects']
            codes = {subject.get('code') or subject.get('name', '') for subject in subjects
                     if (subject.get('authority') or '').lower() == 'lcc'}
            codes = {code for code in codes if re.match(r'^P[A-Z]*(?:\d|\s|$|\x27)', code.strip().upper())}
            if not codes:
                continue
            book_key = table + ':' + identity
            books[book_key] = dict(key=identity, table=table, title=title, author=author,
                                   format='Audiobook' if book_format == 16 else 'EPUB' if book_format == 1 else str(book_format), codes=codes)
            for code in codes:
                catalog_codes[code].add(book_key)
            probes.update(codes)

    references = []
    for entry, start, end, caption, hierarchy, url, filename, page in reference.execute('''
        SELECT e.entry_id,e.start_number,e.end_number,e.caption,e.hierarchy_json,d.url,d.filename,e.page
        FROM reference_entry e JOIN reference_document d USING(document_id)
        WHERE e.status='active' AND e.table_id IS NULL AND
        (e.start_number GLOB 'PQ[0-9]*' OR e.start_number GLOB 'PR[0-9]*'
         OR e.start_number GLOB 'PS[0-9]*' OR e.start_number GLOB 'PT[0-9]*')
        ORDER BY e.entry_id'''):
        codes = {start, end} - {None, ''}
        for code in list(codes):
            probes.add(code)
        # A midpoint catches missing interiors between covered endpoints.
        a, b = CALL.match(start), CALL.match(end or start)
        if a and b and a[1] == b[1] and not a[3].strip() and not b[3].strip() and a[2] != b[2]:
            codes.add(a[1] + format((float(a[2]) + float(b[2])) / 2, '.12g'))
        probes.update(codes)
        references.append(dict(entry_id=entry, start=start, end=end or '', caption=caption,
                               hierarchy=' / '.join(json.loads(hierarchy)), source=url + '#page=' + str(page),
                               document=filename, page=page, codes=codes))

    reviewed = json.loads((DATA / 'literature-cleanup-20261014.json').read_text())['reviewed_scopes']
    for scope in reviewed:
        scope['codes'] = {scope['start'], scope['end']}
        a, b = CALL.match(scope['start']), CALL.match(scope['end'])
        if a and b and a[1] == b[1] and not a[3].strip() and not b[3].strip():
            scope['codes'].add(a[1] + format((float(a[2]) + float(b[2])) / 2, '.12g'))
        probes.update(scope['codes'])

    stored = []
    for identity, selector in tax.execute('SELECT concept_id,selector FROM lcc_selector'):
        if identity in literature:
            probes.add(selector)
            stored.append(dict(concept_id=identity, selector=selector, kind='exact', codes={selector}))
    for identity, sl, sn, sc, el, en, ec in tax.execute('SELECT * FROM lcc_range'):
        if identity not in literature:
            continue
        def endpoint(letters, number, cutters):
            return letters + (format(number, '.12g') if number is not None else '') + ''.join('.' + str(c[0]) + str(c[1]) for c in json.loads(cutters))
        start, end = endpoint(sl, sn, sc), endpoint(el, en, ec)
        codes = {start, end}
        if sl == el and sn is not None and en is not None and sc == ec == '[]':
            codes.add(sl + format((sn + en) / 2, '.12g'))
        probes.update(codes)
        stored.append(dict(concept_id=identity, selector=start + '..' + end, kind='range', codes=codes))

    source = io.StringIO()
    writer = csv.writer(source)
    writer.writerow(['code'])
    writer.writerows([code] for code in sorted(probes))
    result = subprocess.run([str(args.binary), str(frozen['taxonomy'])], input=source.getvalue(), text=True, capture_output=True, check=True)
    rows = list(csv.DictReader(io.StringIO(result.stdout)))
    results = {}
    for row in rows:
        row['ids'] = {int(value) for value in row['resolved_ids'].split('|') if value}
        candidates = {int(value) for value in row['candidate_ids'].split('|') if value}
        row['status'] = ('ambiguous' if len(candidates) > 1 and not row['ids'] else
                         'unmatched' if not row['ids'] else
                         'outside_literature' if not row['ids'] & literature else
                         'broad_literature' if row['ids'] <= broad else 'literature')
        row['paths'] = ' | '.join(sorted({path for identity in row['ids'] for path in paths(identity)}))
        row['subclass_paths'] = ' | '.join(sorted({path for value in row['subclass_ids'].split('|') if value for path in paths(int(value))}))
        results[row['code']] = row
    write_csv(args.output / 'probe-results.csv', results.values(),
              ['code', 'canonical', 'candidate_ids', 'resolved_ids', 'exact_selector', 'subclass_ids', 'unresolved_fragments', 'ids', 'status', 'paths', 'subclass_paths'])
    issue_status = {'unmatched', 'ambiguous', 'outside_literature'}

    reference_numbers = set()
    historical_numbers = set()
    for start, end in reference.execute("SELECT start_number,end_number FROM reference_entry WHERE status='active' AND table_id IS NULL AND start_number GLOB 'P*'"):
        for value in (start, end):
            match = CALL.match(value or '')
            if match:
                reference_numbers.add((match[1], float(match[2])))
    for start, end in reference.execute("SELECT start_number,end_number FROM classification_span WHERE table_id IS NULL AND start_number GLOB 'P*'"):
        for value in (start, end):
            match = CALL.match(value or '')
            if match:
                historical_numbers.add((match[1], float(match[2])))

    def number_evidence(canonical):
        match = CALL.match(canonical)
        if not match:
            return 'no_number'
        letters, number = match[1], float(match[2])
        if number > {'PQ': 9999, 'PR': 9680, 'PS': 9649, 'PT': 9999}.get(letters, float('inf')):
            return 'above_documented_umbrella_review_metadata'
        if (letters, number) in reference_numbers:
            return '2025_reference_number'
        if (letters, number) in historical_numbers:
            return 'retrospective_or_local_reference_number'
        return 'catalog_number_requires_review'

    catalog_rows = []
    for code, keys in sorted(catalog_codes.items()):
        row = results[code]
        sample = sorted({books[key]['title'] for key in keys})
        catalog_rows.append({field: row[field] for field in ['code', 'canonical', 'status', 'resolved_ids', 'candidate_ids', 'paths', 'subclass_paths']} |
                            dict(entries=len(keys), examples=' | '.join(sample[:6]), book_keys=' | '.join(sorted(keys)), number_evidence=number_evidence(row['canonical'])))
    write_csv(args.output / 'catalog-codes.csv', catalog_rows,
              ['code', 'canonical', 'status', 'entries', 'examples', 'resolved_ids', 'candidate_ids', 'paths', 'subclass_paths', 'number_evidence', 'book_keys'])
    core_gaps = [row for row in catalog_rows if row['status'] in issue_status and re.match(r'^(PQ|PR|PS|PT)\s*\d', row['canonical'])]
    write_csv(args.output / 'catalog-literature-gaps.csv', core_gaps,
              ['code', 'canonical', 'status', 'entries', 'examples', 'resolved_ids', 'candidate_ids', 'paths', 'subclass_paths', 'number_evidence', 'book_keys'])

    installed = readonly(frozen['installed'])
    release = installed.execute('SELECT release_id FROM snapshot').fetchone()[0]
    installed_concepts = defaultdict(set)
    for key, concept in installed.execute('SELECT lower(hex(b.key)),bc.concept_id FROM book b JOIN book_concept bc ON bc.book_row_id=b.row_id'):
        installed_concepts[key].add(concept)
    affected = []
    for key, book in sorted(books.items()):
        bad = [code for code in sorted(book['codes']) if results[code]['status'] in issue_status]
        core_bad = [code for code in bad if re.match(r'^(PQ|PR|PS|PT)\s*\d', results[code]['canonical'])]
        if not core_bad:
            continue
        affected.append({field: book[field] for field in ['key', 'table', 'title', 'author', 'format']} |
                        dict(problem_codes=' | '.join(bad), core_literature_problem_codes=' | '.join(core_bad),
                             other_lcc_literature_match=any(results[code]['ids'] & literature for code in book['codes']),
                             installed_only_language=installed_concepts[book['key']] == {3702},
                             installed_concept_ids='|'.join(map(str, sorted(installed_concepts[book['key']])))))
    write_csv(args.output / 'affected-entries.csv', affected,
              ['key', 'table', 'title', 'author', 'format', 'problem_codes', 'core_literature_problem_codes',
               'other_lcc_literature_match', 'installed_only_language', 'installed_concept_ids'])

    ref_rows = []
    for ref in references:
        bad = [(code, results[code]) for code in sorted(ref['codes']) if results[code]['status'] in issue_status]
        for code, row in bad:
            ref_rows.append({field: ref[field] for field in ['entry_id', 'start', 'end', 'caption', 'hierarchy', 'source']} |
                            {field: row[field] for field in ['code', 'status', 'paths', 'subclass_paths']})
    write_csv(args.output / 'reference-gaps.csv', ref_rows,
              ['entry_id', 'start', 'end', 'caption', 'hierarchy', 'code', 'status', 'paths', 'subclass_paths', 'source'])

    groups = defaultdict(list)
    for row in ref_rows:
        # Preserve the reference hierarchy instead of inventing author periods
        # from the numeric code. The CSV's endpoints are observed gap probes,
        # not a proposal to allocate every number between them.
        groups[CALL.match(row['code'])[1], row['hierarchy'].split(' / ')[0]].append(row)
    group_rows = []
    for (subclass, context), group in groups.items():
        codes = {row['code'] for row in group}
        numbers = {float(CALL.match(code)[2]) for code in codes}
        catalog = [row for row in core_gaps if CALL.match(row['canonical'])[1] == subclass and float(CALL.match(row['canonical'])[2]) in numbers]
        entries = {key for row in catalog for key in row['book_keys'].split(' | ')}
        group_rows.append(dict(subclass=subclass, reference_context=context,
                               reference_gap_probes=len(codes), reference_entries=len({row['entry_id'] for row in group}),
                               lowest_number=min(numbers), highest_number=max(numbers),
                               catalog_gap_codes=len(catalog), catalog_entries=len(entries),
                               example_codes=' | '.join(sorted(codes)[:6]),
                               example_titles=' | '.join(sorted({books[key]['title'] for key in entries})[:6]), source=group[0]['source']))
    write_csv(args.output / 'reference-gap-groups.csv', sorted(group_rows, key=lambda row: -row['reference_gap_probes']),
              ['subclass', 'reference_context', 'reference_gap_probes', 'reference_entries', 'lowest_number', 'highest_number',
               'catalog_gap_codes', 'catalog_entries', 'example_codes', 'example_titles', 'source'])

    scope_rows = []
    for scope in reviewed:
        for code in sorted(scope['codes']):
            row = results[code]
            if row['status'] in issue_status:
                scope_rows.append(dict(record_id=scope['record_id'], caption=scope['caption'], start=scope['start'], end=scope['end'], code=code, status=row['status'], paths=row['paths']))
    write_csv(args.output / 'reviewed-scope-gaps.csv', scope_rows,
              ['record_id', 'caption', 'start', 'end', 'code', 'status', 'paths'])

    stored_rows = []
    for selector in stored:
        for code in sorted(selector['codes']):
            row = results[code]
            if row['status'] in issue_status:
                stored_rows.append(dict(concept_id=selector['concept_id'], intended_paths=' | '.join(paths(selector['concept_id'])),
                                        selector=selector['selector'], kind=selector['kind'], code=code, status=row['status'], actual_paths=row['paths']))
    write_csv(args.output / 'stored-rule-gaps.csv', stored_rows,
              ['concept_id', 'intended_paths', 'selector', 'kind', 'code', 'status', 'actual_paths'])

    # Count concrete matching holes in printed spans without treating every
    # unallocated number between two schedule endpoints as a valid LC code.
    inert = []
    for selector in stored:
        match = re.fullmatch(r'([A-Z]+)(\d+(?:\.\d+)?)-([A-Z]+)?(\d+(?:\.\d+)?)', selector['selector'])
        if selector['kind'] != 'exact' or not match or (match[3] and match[1] != match[3]):
            continue
        letters, low, high = match[1], float(match[2]), float(match[4])
        holes = []
        for row in ref_rows:
            call = CALL.match(results[row['code']]['canonical'])
            if call and call[1] == letters and low <= float(call[2]) <= high:
                holes.append(row['code'])
        for row in catalog_rows:
            call = CALL.match(row['canonical'])
            if call and row['status'] in issue_status and call[1] == letters and low <= float(call[2]) <= high:
                holes.append(row['code'])
        inert.append(dict(concept_id=selector['concept_id'], paths=' | '.join(paths(selector['concept_id'])),
                          selector=selector['selector'], uncovered_probe_codes=len(set(holes)), examples=' | '.join(sorted(set(holes))[:8])))
    write_csv(args.output / 'atomic-span-coverage.csv', sorted(inert, key=lambda row: -row['uncovered_probe_codes']),
              ['concept_id', 'paths', 'selector', 'uncovered_probe_codes', 'examples'])

    core_rows = [row for row in catalog_rows if re.match(r'^(PQ|PR|PS|PT)\s*\d', row['canonical'])]
    ref_codes = {code for ref in references for code in ref['codes']}
    summary = dict(
        scope='LCC only; current curated catalog P-class codes; active 2025 PQ/PR/PS/PT reference endpoints and numeric midpoints; 228 reviewed mixed-language literary scopes; all Literature subtree rules',
        limitations=['A missing match is not proof a catalog classification is bibliographically correct.',
                     'Reference probes do not enumerate every possible Cutter or validate interpolated midpoint allocations.',
                     'Broad literary matches are listed separately and are not automatically missing coverage.',
                     'Mixed language/literature subclasses are audited via the reviewed scopes, stored literary rules and catalog evidence, not every reference entry.',
                     'Entry impact is based on individual LCC matching; other classification systems or other LCC codes may still assign the book.'],
        input_paths={name: str(getattr(args, name)) for name in frozen},
        input_sha256={name: fingerprint(path) for name, path in frozen.items()},
        matcher_revision=result.stderr.strip(), installed_release=release,
        literature_concepts=len(literature), unique_probes=len(probes),
        catalog_entries_with_p_lcc=len(books), catalog_unique_p_codes=len(catalog_codes),
        catalog_code_status=dict(Counter(row['status'] for row in catalog_rows)),
        core_catalog_code_status=dict(Counter(row['status'] for row in core_rows)),
        catalog_entries_with_core_literature_problem=sum(bool(row['core_literature_problem_codes']) for row in affected),
        catalog_entries_only_language_with_core_problem=sum(row['installed_only_language'] and bool(row['core_literature_problem_codes']) for row in affected),
        catalog_entries_with_core_problem_without_other_literary_lcc_match=sum(not row['other_lcc_literature_match'] for row in affected),
        core_catalog_gap_number_evidence=dict(Counter(row['number_evidence'] for row in core_gaps)),
        core_catalog_gaps_by_subclass={subclass: dict(codes=len(subset), entries=len({key for row in subset for key in row['book_keys'].split(' | ')}))
                                      for subclass in sorted(CORE) if (subset := [row for row in core_gaps if CALL.match(row['canonical'])[1] == subclass])},
        reference_entries=len(references), reference_unique_probes=len(ref_codes),
        reference_probe_status=dict(Counter(results[code]['status'] for code in ref_codes)),
        reference_entries_with_gaps=len({row['entry_id'] for row in ref_rows}),
        reference_gap_probes_by_subclass=dict(Counter(CALL.match(results[code]['canonical'])[1] for code in {row['code'] for row in ref_rows} if CALL.match(results[code]['canonical']))),
        reviewed_scopes=len(reviewed), reviewed_scope_gap_probes=len(scope_rows),
        stored_rule_gap_probes=len(stored_rows), atomic_spans=len(inert),
    )
    (args.output / 'summary.json').write_text(json.dumps(summary, ensure_ascii=False, indent=2) + '\n')
    print(json.dumps(summary, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    main()
