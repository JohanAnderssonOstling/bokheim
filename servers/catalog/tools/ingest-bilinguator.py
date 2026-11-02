#!/usr/bin/env python3
"""Survey Bilinguator rights, then acquire attributed bilingual EPUBs.

Requires beautifulsoup4. Keeps source responses, originals, receipts, and the
attributed copies separately. No catalog writes or publication are implicit.
"""
import argparse
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
from copy import copy
from datetime import datetime, timezone
import hashlib
from html import escape
import io
from itertools import combinations
import json
from pathlib import Path
import posixpath
import re
import shutil
import time
from urllib.parse import parse_qs, urlencode, urljoin, urlparse
from urllib.request import Request, urlopen
import xml.etree.ElementTree as ET
import zipfile

from bs4 import BeautifulSoup

ORIGIN = 'https://bilinguator.com'
# Operator-reviewed subject exclusions apply to every language pairing of the
# work, including pairs that do not contain an Esperanto translation.
EXCLUDED_WORKS = {'manifesto_de_prago': 'Operator excluded works about Esperanto'}
EXCLUDED_LANGUAGES = {'eo', 'epo'}
UNSUPPORTED_LANGUAGE_POLICY = json.loads(Path(__file__).with_name('excluded-catalog-languages.json').read_text())
DC = 'http://purl.org/dc/elements/1.1/'
OPF = 'http://www.idpf.org/2007/opf'
ET.register_namespace('', OPF)
ET.register_namespace('dc', DC)


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + '.partial')
    temporary.write_text(json.dumps(value, ensure_ascii=False, indent=2) + '\n')
    temporary.replace(path)


def digest(data):
    return hashlib.sha256(data).hexdigest()


class Source:
    def __init__(self, directory, delay=0.35):
        self.directory, self.delay = directory, delay
        self.cache = directory / 'pages'
        self.cache.mkdir(parents=True, exist_ok=True)

    def request(self, url, data=None):
        if urlparse(url).scheme != 'https' or urlparse(url).hostname != 'bilinguator.com':
            raise ValueError('Expected an HTTPS Bilinguator URL')
        time.sleep(self.delay)
        request = Request(url, data=data, headers={'User-Agent': 'bokheim-catalog', 'Referer': ORIGIN + '/en/'})
        with urlopen(request, timeout=90) as response:
            if urlparse(response.url).hostname != 'bilinguator.com':
                raise ValueError('Unexpected source redirect: ' + response.url)
            return response.read()

    def fetch(self, url):
        path = self.cache / (digest(url.encode()) + '.html')
        if not path.exists():
            data = self.request(url)
            path.write_bytes(data)
            write_json(path.with_suffix('.json'), dict(url=url, sha256=digest(data),
                       observed_at=datetime.now(timezone.utc).isoformat()))
        data = path.read_bytes()
        receipt_path = path.with_suffix('.json')
        receipt = json.loads(receipt_path.read_text()) if receipt_path.exists() else {}
        if receipt.get('sha256') != digest(data) or receipt.get('url') != url:
            raise ValueError('Missing or mismatched cached source receipt: ' + str(path))
        if 'observed_at' not in receipt:
            receipt['observed_at'] = datetime.fromtimestamp(path.stat().st_mtime, timezone.utc).isoformat()
            write_json(receipt_path, receipt)
        return data

    def soup(self, url):
        return BeautifulSoup(self.fetch(url), 'html.parser')


def license_for(notice):
    """Fail closed on missing/ambiguous declarations; do not infer from age."""
    text, links = notice.get('text', ''), notice.get('links', [])
    if 'in the public domain and not protected by copyright' in text:
        death = re.search(r"death in (\d+)( BC)?", text)
        # The source also labels life-plus-50 editions public domain. Those
        # labels alone do not establish expiry for this Swedish catalog.
        if death and (death[2] or int(death[1]) + 70 < datetime.now(timezone.utc).year):
            return {'id': 'public-domain', 'url': None}
        return None
    if 'distributed under the CC0' in text:
        return {'id': 'CC0-1.0', 'url': 'https://creativecommons.org/publicdomain/zero/1.0/'}
    match = re.search(r'distributed under the (CC BY(?:-NC)?(?:-SA|-ND)? [34]\.0) license', text)
    if match:
        slug = match[1][3:-4].lower()
        version = match[1][-3:]
        url = f'https://creativecommons.org/licenses/{slug}/{version}/'
        if url not in links:
            return None
        return {'id': match[1].replace(' ', '-'), 'url': url}
    if 'distributed under the GNU Free Documentation License' in text and 'https://www.gnu.org/licenses/fdl-1.3.html' in links:
        return {'id': 'GFDL-1.3', 'url': 'https://www.gnu.org/licenses/fdl-1.3.html'}
    return None


def notices_from(soup, url):
    return [dict(text=node.get_text(' ', strip=True), links=[a['href'] for a in node.select('a[href]')],
                 icon=node.select_one('img')['src'] if node.select_one('img') else None, url=url)
            for node in soup.select('.copyright-message')]


def survey(source):
    urls = {a['href'] for a in source.soup(ORIGIN + '/en/books').select('a[href]') if '/en/book?book=' in a['href']}
    index = ET.fromstring(source.fetch(ORIGIN + '/sitemap.xml'))
    for sitemap in index.iter('{http://www.sitemaps.org/schemas/sitemap/0.9}loc'):
        root = ET.fromstring(source.fetch(sitemap.text))
        urls.update(node.text for node in root.iter('{http://www.sitemaps.org/schemas/sitemap/0.9}loc') if '/en/book?book=' in node.text)
    works = []
    for url in sorted(urls):
        work_id = parse_qs(urlparse(url).query)['book'][0]
        soup = source.soup(url)
        title = soup.select_one('.ind-book-title')
        languages = {n['lingua']: n.get_text(' ', strip=True) for n in soup.select('[lingua]')}
        if title is None or len(languages) < 2:
            works.append(dict(id=work_id, url=url, title=title.get_text(' ', strip=True) if title else '',
                              languages=languages, rights={}, pairs=[], errors=['Fewer than two available languages']))
            write_json(source.directory / 'survey.json', works)
            continue
        author = soup.select_one('.ind-book-author')
        description = soup.select_one('.ind-description')
        original = soup.select_one('.ind-info-book a[href*="language="]')
        original = parse_qs(urlparse(original['href']).query)['language'][0] if original else ''
        base = original if original in languages else next(iter(languages))
        year = next((re.search(r'\b(\d{4})\b', n.get_text()).group(1) for n in soup.select('.ind-info-item')
                     if 'Date of publication:' in n.get_text() and re.search(r'\b(\d{4})\b', n.get_text())), None)
        work = dict(id=work_id, url=url, title=title.get_text(' ', strip=True), author=author.get_text(' ', strip=True) if author else '',
                    description=description.get_text(' ', strip=True) if description else '', year=int(year) if year else None,
                    languages=languages, rights={}, pairs=[], errors=[])
        for lang in languages:
            if lang == base:
                continue
            try:
                listing = source.soup(ORIGIN + '/templates/blocks/load_book_list.php?' + urlencode(dict(book=work_id, lingua1=base, lingua2=lang, lang='en')))
                link = listing.select_one('a.cover-heading-href')
                if link is None:
                    raise ValueError('No advertised bilingual edition')
                pair_url = urljoin(ORIGIN + '/en/', link['href'])
                pair = source.soup(pair_url)
                notices = notices_from(pair, pair_url)
                for code, name in languages.items():
                    matches = [n for n in notices if f'The original {name} text' in n['text'] or f'The {name} translation' in n['text']]
                    if len(matches) == 1:
                        previous = work['rights'].get(code)
                        if previous and previous['text'] != matches[0]['text']:
                            raise ValueError('Conflicting rights declarations for ' + code)
                        work['rights'][code] = matches[0]
                work['pairs'].append(dict(languages=[base, lang], url=pair_url, notices=notices))
            except Exception as error:
                work['errors'].append(dict(language=lang, error=str(error)))
        works.append(work)
        write_json(source.directory / 'survey.json', works)
        print(work_id, len(languages), 'languages;', len(work['rights']), 'rights notices', flush=True)
    return works


def language_exclusion_reason(code):
    base = re.split(r'[-_.@]', code.strip(), maxsplit=1)[0].lower()
    if base in EXCLUDED_LANGUAGES:
        return 'Operator excluded editions containing Esperanto'
    if base in UNSUPPORTED_LANGUAGE_POLICY['aliases']:
        return 'Operator excluded editions containing unsupported language: ' + UNSUPPORTED_LANGUAGE_POLICY['aliases'][base]
    return None


def selection(works):
    selected, held = [], []
    for work in works:
        for first, second in combinations(sorted(work['languages']), 2):
            codes = [first, second]
            notices = [work['rights'].get(code, {}) for code in codes]
            licenses = [license_for(notice) for notice in notices]
            row = dict(work_id=work['id'], languages=codes, entry_id=f"{work['id']}_{first}_{second}",
                       title=work['title'], entry_url=work['url'], notices=notices, licenses=licenses)
            language_reason = next((reason for code in codes if (reason := language_exclusion_reason(code))), None)
            if work['id'] in EXCLUDED_WORKS:
                held.append({**row, 'reason': EXCLUDED_WORKS[work['id']]})
            elif language_reason:
                held.append({**row, 'reason': language_reason})
            elif all(licenses):
                selected.append(row)
            else:
                held.append({**row, 'reason': 'Missing or unsupported redistribution declaration'})
    return selected, held


def catalog_description(work, row):
    credits = []
    for code, notice, info in zip(row['languages'], row['notices'], row['licenses']):
        match = re.search(r' authored by (.*?) is/are ', notice['text'])
        name = match[1] if match else ''
        label = work['languages'][code] + (': ' + name if name else '')
        credits.append(label + ' — ' + info['id'] + ('. ' + info['url'] if info['url'] else '. Copyright expiry varies by country.'))
    return '\n\n'.join(part for part in [work.get('description', ''), 'Credits and licenses\n' + '\n'.join(credits),
                           'Bilingual edition: Bilinguator. ' + work['url']] if part)


def apply_rights_reviews(source, works, path):
    """Apply individually researched expiry reviews bound to exact source text."""
    by_id = {work['id']: work for work in works}
    for review in json.loads(path.read_text()):
        work = by_id[review['work_id']]
        code = review['language']
        if code not in work['languages'] or not review['authority_urls']:
            raise ValueError('Review needs an available language and supporting authorities')
        expected = f"{ORIGIN}/texts/{work['id']}/{work['id']}_{code}.txt"
        if review['source_text_url'] != expected:
            raise ValueError('Review source URL does not identify the selected translation')
        data = source.fetch(expected)
        if digest(data) != review['source_text_sha256'] or review['author'] not in '\n'.join(data.decode('utf-8-sig').splitlines()[:2]):
            raise ValueError('Review does not match the retained text and its credit')
        kind = 'original ' + work['languages'][code] + ' text' if review['kind'] == 'original' else work['languages'][code] + ' translation'
        text = (f"The {kind} of the book “{work['title']}” authored by {review['author']} is/are in the public domain and not protected by copyright "
                f"under the reviewed life-plus-70 term because the author’s death in {review['death_year']} was more than 70 complete years ago. "
                'This expiry review was researched by Bokheim; it is not a Bilinguator license declaration.')
        notice = dict(text=text, links=review['authority_urls'], url=expected, review=review)
        if license_for(notice) is None:
            raise ValueError('Review does not establish life-plus-70 expiry')
        if license_for(work['rights'].get(code, {})) is not None:
            raise ValueError('Review unexpectedly overrides an already supported declaration')
        work['rights'][code] = notice
    return works


def package_info(data, identity, languages):
    archive = zipfile.ZipFile(io.BytesIO(data))
    if archive.testzip() is not None or archive.read('mimetype') != b'application/epub+zip':
        raise ValueError('Invalid EPUB archive')
    container = ET.fromstring(archive.read('META-INF/container.xml'))
    package = container.find('.//{urn:oasis:names:tc:opendocument:xmlns:container}rootfile').get('full-path')
    root = ET.fromstring(archive.read(package))
    metadata = root.find(f'{{{OPF}}}metadata')
    if identity not in [n.text for n in metadata.findall(f'{{{DC}}}identifier')]:
        raise ValueError('Downloaded EPUB has a different identity')
    if set(languages) != {n.text for n in metadata.findall(f'{{{DC}}}language')}:
        raise ValueError('Downloaded EPUB has different languages')
    if metadata.find(f'{{{DC}}}title') is None:
        raise ValueError('Downloaded EPUB has no title')
    return archive, package, root, metadata


def attributed_epub(data, row, work, license_texts):
    """Retain every original resource except OPF; append a separate notice page."""
    archive, package, root, metadata = package_info(data, row['entry_id'], row['languages'])
    prefix = posixpath.dirname(package)
    notice_name = 'bokheim-rights.xhtml'
    if posixpath.join(prefix, notice_name) in archive.namelist():
        raise ValueError('Source EPUB already has our notice path')
    title = metadata.find(f'{{{DC}}}title').text
    original_creators = [n.text for n in metadata.findall(f'{{{DC}}}creator')]
    # The source repeats one author's translated names with slashes. Retain that
    # observation in provenance while using its English work-page credit.
    if work.get('author'):
        for node in list(metadata.findall(f'{{{DC}}}creator')):
            metadata.remove(node)
        ET.SubElement(metadata, f'{{{DC}}}creator').text = work['author']
    sections = []
    translators = []
    for code, notice, license_info in zip(row['languages'], row['notices'], row['licenses']):
        sections.append('<p>' + escape(notice['text']) + '</p><p>Source: <a href="' + escape(notice['url'], quote=True) + '">' + escape(notice['url']) + '</a></p>')
        if notice.get('review'):
            sections.append('<p>Expiry review sources: ' + '; '.join('<a href="' + escape(url, quote=True) + '">' + escape(url) + '</a>'
                            for url in notice['review']['authority_urls']) + '</p>')
        if license_info['url']:
            sections.append('<p>License: <a href="' + escape(license_info['url'], quote=True) + '">' + escape(license_info['id']) + '</a></p>')
            sections.append('<pre>' + escape(license_texts[license_info['id']]) + '</pre>')
        if ' translation of the book ' in notice['text']:
            match = re.search(r' authored by (.*?) is/are ', notice['text'])
            if match and match[1] not in translators:
                translators.append(match[1])
                ET.SubElement(metadata, f'{{{DC}}}contributor', {f'{{{OPF}}}role': 'trl'}).text = match[1]
        ET.SubElement(metadata, f'{{{DC}}}rights').text = code + ': ' + notice['text']
    sections.append('<p>Bilingual edition supplied by Bilinguator: <a href="' + escape(work['url'], quote=True) + '">' + escape(work['url']) + '</a>.</p>')
    sections.append('<p>Bokheim added this rights page and normalized the author metadata. The literary text, translations, images, and original credits are unchanged. The translations retain their respective licenses; this collection does not relicense them.</p>')
    html = ('<?xml version="1.0" encoding="utf-8"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>Credits and licenses</title></head>'
            '<body><h1>Credits and licenses</h1><h2>' + escape(title) + '</h2>' + ''.join(sections) + '</body></html>').encode()
    ET.fromstring(html)
    manifest = root.find(f'{{{OPF}}}manifest')
    spine = root.find(f'{{{OPF}}}spine')
    if any(n.get('id') == 'bokheim-rights' for n in manifest):
        raise ValueError('Conflicting notice manifest ID')
    ET.SubElement(manifest, f'{{{OPF}}}item', dict(id='bokheim-rights', href=notice_name, **{'media-type': 'application/xhtml+xml'}))
    ET.SubElement(spine, f'{{{OPF}}}itemref', dict(idref='bokheim-rights'))
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, 'w') as output:
        for member in archive.infolist():
            output.writestr(copy(member), ET.tostring(root, encoding='utf-8', xml_declaration=True) if member.filename == package else archive.read(member.filename))
        output.writestr(posixpath.join(prefix, notice_name), html, compress_type=zipfile.ZIP_DEFLATED)
    result = buffer.getvalue()
    with zipfile.ZipFile(io.BytesIO(result)) as check:
        assert check.testzip() is None
        for name in archive.namelist():
            if name != package:
                assert archive.read(name) == check.read(name), name
    return result, dict(title=title, source_creators=original_creators, translators=translators,
                        original_resources_preserved=len(archive.namelist()) - 1, notice_resource=posixpath.join(prefix, notice_name))


def download(source, works, workers=1):
    selected, held = selection(works)
    write_json(source.directory / 'selection.json', selected)
    write_json(source.directory / 'held.json', held)
    licenses = {}
    license_directory = source.directory / 'licenses'
    license_directory.mkdir(exist_ok=True)
    for row in selected:
        for info in row['licenses']:
            if not info['url'] or info['id'] in licenses:
                continue
            path = license_directory / (info['id'] + '.txt')
            if not path.exists():
                if info['id'] == 'GFDL-1.3':
                    url = 'https://www.gnu.org/licenses/fdl-1.3.txt'
                else:
                    url = info['url'].rstrip('/') + '/legalcode.txt'
                time.sleep(source.delay)
                with urlopen(Request(url, headers={'User-Agent': 'bokheim-catalog'}), timeout=90) as response:
                    data = response.read()
                if b'<html' in data[:500].lower() or len(data) < 1000:
                    raise ValueError('Expected full license text: ' + url)
                path.write_bytes(data)
            licenses[info['id']] = path.read_text()
    by_id = {work['id']: work for work in works}
    outcomes_path = source.directory / 'downloads.json'
    outcomes = json.loads(outcomes_path.read_text()) if outcomes_path.exists() else {}
    manifest = []
    def acquire(row):
        identity = row['entry_id']
        file_url = f'{ORIGIN}/downloads/{identity}.epub'
        original = source.directory / 'originals' / (identity + '.epub')
        derivative = source.directory / 'attributed' / original.name
        original.parent.mkdir(exist_ok=True)
        derivative.parent.mkdir(exist_ok=True)
        try:
            if shutil.disk_usage(source.directory).free < 1024 ** 3:
                raise OSError('Less than 1 GiB local storage remains; acquisition stopped')
            if not original.exists():
                first, second = row['languages']
                source.request(ORIGIN + '/templates/save/save_epub.php', urlencode(dict(id=row['work_id'], lingua1=first, lingua2=second)).encode())
                data = source.request(file_url)
                package_info(data, identity, row['languages'])
                temporary = original.with_suffix('.partial')
                temporary.write_bytes(data)
                temporary.replace(original)
            data = original.read_bytes()
            previous = outcomes.get(identity, {})
            if previous.get('source_sha256') and previous['source_sha256'] != digest(data):
                raise ValueError('Retained source EPUB changed')
            if not derivative.exists() or previous.get('state') != 'downloaded':
                attributed, receipt = attributed_epub(data, row, by_id[row['work_id']], licenses)
                derivative.write_bytes(attributed)
            else:
                attributed = derivative.read_bytes()
                if previous['sha256'] != digest(attributed):
                    raise ValueError('Retained attributed EPUB changed')
                receipt = previous['attribution']
            evidence = dict(kind='bilinguator_bilingual', work_id=row['work_id'], languages=row['languages'],
                            licenses=row['licenses'], copyright_notices=row['notices'], source_sha256=digest(data),
                            sha256=digest(attributed), attribution=receipt, redistribution_context='noncommercial',
                            source_file_url=file_url, observed_at=previous.get('observed_at', datetime.now(timezone.utc).isoformat()))
            outcome = {**evidence, 'state': 'downloaded', 'bytes': len(attributed)}
            work = by_id[row['work_id']]
            entry = dict(file=str(derivative.relative_to(source.directory)), source='bilinguator.com', entry_id=identity,
                                 entry_url=work['url'], file_url=file_url,
                                 metadata=dict(title=receipt['title'], description=catalog_description(work, row), languages=row['languages']),
                                 source_evidence=evidence)
            return identity, outcome, entry
        except Exception as error:
            if isinstance(error, OSError) and 'storage' in str(error):
                raise
            return identity, dict(state='failed', error=str(error)), None
    with ThreadPoolExecutor(max_workers=workers) as pool:
        for index, (identity, outcome, entry) in enumerate(pool.map(acquire, selected)):
            outcomes[identity] = outcome
            if entry:
                manifest.append(entry)
                print(index + 1, '/', len(selected), identity, outcome['bytes'], 'bytes', flush=True)
            else:
                print(identity, 'FAILED:', outcome['error'], flush=True)
            # Commit small acquisition batches; original files remain resumable
            # between checkpoints without repeatedly rewriting large manifests.
            if (index + 1) % 10 == 0 or index + 1 == len(selected):
                write_json(outcomes_path, outcomes)
                write_json(source.directory / 'import-manifest.json', manifest)
    write_json(source.directory / 'summary.json', dict(works=len(works), selected_pairs=len(selected), held_pairs=len(held),
               acquired=len(manifest), failures=sum(v['state'] == 'failed' for v in outcomes.values()),
               licenses=dict(Counter(info['id'] for row in selected for info in row['licenses']))))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=['survey', 'download'])
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--delay', type=float, default=0.35)
    parser.add_argument('--workers', type=int, choices=range(1, 5), default=1)
    parser.add_argument('--rights-review', type=Path)
    args = parser.parse_args()
    source = Source(args.output.resolve(), args.delay)
    if args.command == 'survey':
        survey(source)
    else:
        works = json.loads((source.directory / 'survey.json').read_text())
        if args.rights_review:
            works = apply_rights_reviews(source, works, args.rights_review)
        download(source, works, args.workers)


if __name__ == '__main__':
    main()
