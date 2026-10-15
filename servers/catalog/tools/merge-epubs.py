#!/usr/bin/env python3
"""Combine explicitly ordered EPUB2/3 volumes into EPUB3, preserving text and assets."""
import argparse
from copy import deepcopy
from dataclasses import dataclass
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import posixpath
import re
import tempfile
from urllib.parse import quote, unquote, urlsplit, urlunsplit
import uuid
import zipfile
import xml.etree.ElementTree as ET
from xml.parsers import expat


OPF = 'http://www.idpf.org/2007/opf'
DC = 'http://purl.org/dc/elements/1.1/'
XHTML = 'http://www.w3.org/1999/xhtml'
EPUB = 'http://www.idpf.org/2007/ops'
CONTAINER = 'urn:oasis:names:tc:opendocument:xmlns:container'
XML = 'http://www.w3.org/XML/1998/namespace'
NCX = 'http://www.daisy.org/z3986/2005/ncx/'
CSS_URL = re.compile(r'url\(\s*[\'"]?([^\s\)\'"]+)|@import\s+[\'"]([^\'"]+)', re.I)
ET.register_namespace('dc', DC)
ET.register_namespace('epub', EPUB)


def tag(namespace, name):
    return '{' + namespace + '}' + name


def xml_bytes(element):
    return ET.tostring(element, encoding='utf-8', xml_declaration=True)


def valid_modified(value):
    if not re.fullmatch(r'[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z', value):
        return False
    try:
        datetime.strptime(value, '%Y-%m-%dT%H:%M:%SZ')
    except ValueError:
        return False
    return True


def first_source_modified(volumes):
    for volume in volumes:
        for element in volume.package.find(tag(OPF, 'metadata')):
            if (element.tag == tag(OPF, 'meta') and element.get('property') == 'dcterms:modified'
                    and not element.get('refines')):
                value = (element.text or '').strip()
                if valid_modified(value):
                    return value
        if volume.package.get('version') == '2.0':
            timestamp = legacy_timestamp(volume.package)
            if timestamp is not None:
                return timestamp['normalized']
    raise ValueError('No valid source last-modified timestamp; supply --modified explicitly')


def legacy_timestamp(package):
    """Reuse an intrinsic EPUB2 date, never the current time or file mtime."""
    metadata = package.find(tag(OPF, 'metadata'))
    candidates = []
    for element in metadata:
        if element.tag == tag(DC, 'date') and element.get(tag(OPF, 'event')) == 'modification':
            candidates.append(('dc:date[opf:event=modification]', element.text or ''))
    for element in metadata:
        if element.tag == tag(OPF, 'meta') and element.get('name') in ('dcterms:modified', 'calibre:timestamp'):
            candidates.append((element.get('name'), element.get('content', '')))
    candidates.extend(('dc:date', element.text or '') for element in metadata if element.tag == tag(DC, 'date'))
    for field, value in candidates:
        try:
            parsed = datetime.fromisoformat(value.strip().replace('Z', '+00:00'))
            if parsed.tzinfo is None:
                continue
            normalized = parsed.astimezone(timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')
            if valid_modified(normalized):
                return {'field': field, 'original_value': value, 'normalized': normalized}
        except ValueError:
            continue
    return None


def local_target(origin, value):
    """Resolve an archive URI, rejecting paths outside the publication."""
    uri = urlsplit(value)
    if uri.scheme or uri.netloc:
        return None
    path = unquote(uri.path)
    if path.startswith('/'):
        raise ValueError(f'Archive-root links are not supported: {origin}: {value}')
    resolved = posixpath.normpath(posixpath.join(posixpath.dirname(origin), path)) if path else origin
    if resolved.startswith('../') or resolved in ('.', '..'):
        raise ValueError(f'Link escapes the publication: {origin}: {value}')
    return resolved


def relocated_uri(origin, value, prefix):
    target = local_target(origin, value)
    if target is None:
        raise ValueError(f'Navigation and manifest resources must be local: {value}')
    uri = urlsplit(value)
    return urlunsplit(('', '', quote(prefix + target, safe='/'), uri.query, uri.fragment))


def navigation_entries(list_element, origin, prefix):
    entries = []
    if list_element is None:
        return entries
    for li in list_element.findall(tag(XHTML, 'li')):
        label = next((child for child in li if child.tag in (tag(XHTML, 'a'), tag(XHTML, 'span'))), None)
        if label is None:
            raise ValueError(f'Navigation entry has no label: {origin}')
        text = ' '.join(''.join(label.itertext()).split())
        if not text:
            raise ValueError(f'Navigation entry has an empty label: {origin}')
        href = label.get('href')
        entries.append({'label': text, 'href': relocated_uri(origin, href, prefix) if href else None,
                        'children': navigation_entries(li.find(tag(XHTML, 'ol')), origin, prefix)})
    return entries


def ncx_entries(parent, origin, prefix):
    entries = []
    for point in parent.findall(tag(NCX, 'navPoint')):
        label = point.find(tag(NCX, 'navLabel'))
        content = point.find(tag(NCX, 'content'))
        text = ' '.join(''.join(label.itertext()).split()) if label is not None else ''
        if not text or content is None or not content.get('src'):
            raise ValueError('EPUB2 NCX entry needs its original label and local target')
        entries.append({'label': text, 'href': relocated_uri(origin, content.get('src'), prefix),
                        'children': ncx_entries(point, origin, prefix)})
    return entries


@dataclass
class Volume:
    path: Path
    label: str
    prefix: str
    sha256: str
    files: dict
    package: ET.Element
    package_path: str
    title: str
    creators: tuple
    contributors: tuple
    languages: tuple
    toc: list
    pages: list


def read_volume(path, label, index):
    path = Path(path)
    data = path.read_bytes()
    with zipfile.ZipFile(path) as archive:
        names = [item.filename for item in archive.infolist() if not item.is_dir()]
        if len(names) != len(set(names)):
            raise ValueError(f'Duplicate archive filenames: {path}')
        if any(name.startswith('/') or '\\' in name or posixpath.normpath(name) != name or name.startswith('../') for name in names):
            raise ValueError(f'Unsafe archive filenames: {path}')
        files = {name: archive.read(name) for name in names}
    if files.get('mimetype') != b'application/epub+zip':
        raise ValueError(f'Not an EPUB: {path}')
    for special in ['META-INF/encryption.xml', 'META-INF/signatures.xml']:
        if special in files:
            raise ValueError(f'Cannot merge encrypted or signed resources: {path}: {special}')
    container = ET.fromstring(files['META-INF/container.xml'])
    roots = container.findall('.//' + tag(CONTAINER, 'rootfile'))
    if len(roots) != 1:
        raise ValueError(f'Exactly one rendition is required: {path}')
    package_path = roots[0].get('full-path')
    package = ET.fromstring(files[package_path])
    legacy = package.get('version') == '2.0'
    if not legacy and not package.get('version', '').startswith('3.'):
        raise ValueError(f'Only EPUB2 and EPUB3 sources are supported: {path}')
    if any(element.get(tag(XML, 'base')) is not None for element in package.iter()):
        raise ValueError(f'Package xml:base is not supported: {path}')
    metadata = package.find(tag(OPF, 'metadata'))
    manifest = package.find(tag(OPF, 'manifest'))
    spine = package.find(tag(OPF, 'spine'))
    if metadata is None or manifest is None or spine is None or len(spine) == 0:
        raise ValueError(f'Missing metadata, manifest or reading order: {path}')
    if any(item.get('media-overlay') or item.get('media-type') == 'application/smil+xml' for item in manifest):
        raise ValueError(f'Synchronized audio requires a separate merge strategy: {path}')
    if spine.get('page-progression-direction', 'default') not in ('default', 'ltr'):
        raise ValueError(f'Only left-to-right reading order is supported: {path}')
    if any(e.text == 'pre-paginated' for e in metadata if e.get('property') == 'rendition:layout'):
        raise ValueError(f'Fixed-layout volumes require a separate merge strategy: {path}')
    prefix = f'volumes/v{index:03}/'
    if legacy:
        ncx_items = [item for item in manifest if item.get('media-type') == 'application/x-dtbncx+xml']
        referenced = [item for item in ncx_items if item.get('id') == spine.get('toc')]
        selected = referenced or ncx_items
        if len(selected) != 1:
            raise ValueError(f'EPUB2 source requires one unambiguous original NCX: {path}')
        nav_path = local_target(package_path, selected[0].get('href'))
        navigation = ET.fromstring(files[nav_path])
        nav_map = navigation.find(tag(NCX, 'navMap'))
        if nav_map is None:
            raise ValueError(f'EPUB2 source has no original NCX navMap: {path}')
        entries = ncx_entries(nav_map, nav_path, prefix)
        pages = []
        page_list = navigation.find(tag(NCX, 'pageList'))
        for point in page_list if page_list is not None else []:
            content, label_node = point.find(tag(NCX, 'content')), point.find(tag(NCX, 'navLabel'))
            page_label = ' '.join(''.join(label_node.itertext()).split()) if label_node is not None else ''
            if content is None or not content.get('src') or not page_label:
                raise ValueError('EPUB2 NCX page needs its original label and local target')
            pages.append({'label': page_label, 'href': relocated_uri(nav_path, content.get('src'), prefix), 'children': []})
    else:
        nav_items = [item for item in manifest if 'nav' in item.get('properties', '').split()]
        if len(nav_items) != 1:
            raise ValueError(f'Exactly one EPUB3 navigation document is required: {path}')
        nav_path = local_target(package_path, nav_items[0].get('href'))
        navigation = ET.fromstring(files[nav_path])
        navs = list(navigation.iter(tag(XHTML, 'nav')))
        toc = [nav for nav in navs if 'toc' in nav.get(tag(EPUB, 'type'), '').split()]
        if len(toc) != 1:
            raise ValueError(f'Missing or ambiguous table of contents: {path}')
        entries = navigation_entries(toc[0].find(tag(XHTML, 'ol')), nav_path, prefix)
        pages = [entry for nav in navs if 'page-list' in nav.get(tag(EPUB, 'type'), '').split()
                 for entry in navigation_entries(nav.find(tag(XHTML, 'ol')), nav_path, prefix)]
    if any(e.get(tag(XML, 'base')) is not None for e in navigation.iter()):
        raise ValueError(f'Navigation xml:base is not supported: {path}')
    if not entries:
        raise ValueError(f'Empty table of contents: {path}')
    return Volume(path, label, prefix, hashlib.sha256(data).hexdigest(), files, package, package_path,
                  metadata.findtext(tag(DC, 'title'), '').strip(),
                  tuple(sorted(' '.join(e.itertext()).strip() for e in metadata.findall(tag(DC, 'creator')))),
                  tuple(sorted(' '.join(e.itertext()).strip() for e in metadata.findall(tag(DC, 'contributor')))),
                  tuple(sorted(e.text.strip() for e in metadata.findall(tag(DC, 'language')) if e.text)), entries, pages)


def add_list(parent, entries):
    ordered = ET.SubElement(parent, tag(XHTML, 'ol'))
    for entry in entries:
        item = ET.SubElement(ordered, tag(XHTML, 'li'))
        label = ET.SubElement(item, tag(XHTML, 'a') if entry['href'] else tag(XHTML, 'span'))
        if entry['href']:
            label.set('href', entry['href'])
        label.text = entry['label']
        if entry['children']:
            add_list(item, entry['children'])


def expand_numbered_sections(volume):
    """Add explicitly marked § paragraphs; edit only missing id attributes."""
    positions = {}

    def index(entries, parent=None):
        for entry in entries:
            if entry['href']:
                uri = urlsplit(entry['href'])
                name = unquote(uri.path).removeprefix(volume.prefix)
                positions[(name, unquote(uri.fragment))] = (entry, parent)
            index(entry['children'], entry)

    index(volume.toc)
    manifest = {item.get('id'): item for item in volume.package.find(tag(OPF, 'manifest'))}
    current = None
    candidates = []
    reading_positions = {}
    for spine_position, itemref in enumerate(volume.package.find(tag(OPF, 'spine'))):
        item = manifest[itemref.get('idref')]
        if item.get('media-type') != 'application/xhtml+xml' or itemref.get('linear', 'yes') == 'no':
            continue
        name = local_target(volume.package_path, item.get('href'))
        data = volume.files[name]
        reading_positions.setdefault((name, ''), (spine_position, 0))
        parser = expat.ParserCreate(namespace_separator='}')
        paragraph = None

        def start(element, attributes):
            nonlocal current, paragraph
            if attributes.get('id'):
                reading_positions.setdefault((name, attributes['id']), (spine_position, parser.CurrentByteIndex))
            if attributes.get('id') and (name, attributes['id']) in positions:
                current = positions[(name, attributes['id'])]
            if element == XHTML + '}p':
                paragraph = {'position': parser.CurrentByteIndex, 'spine_position': spine_position,
                             'id': attributes.get('id'), 'text': [], 'branch': current}

        def characters(value):
            if paragraph is not None:
                paragraph['text'].append(value)

        def end(element):
            nonlocal paragraph
            if element != XHTML + '}p' or paragraph is None:
                return
            text = ''.join(paragraph['text'])
            match = re.match(r'^\s*§\s*(\d+)\.', text)
            if match and paragraph['branch'] is not None:
                candidates.append({**paragraph, 'number': int(match[1]), 'name': name})
            paragraph = None

        parser.StartElementHandler = start
        parser.CharacterDataHandler = characters
        parser.EndElementHandler = end
        parser.Parse(data, True)
    if not candidates:
        return {}, []
    numbers = [candidate['number'] for candidate in candidates]
    if len(numbers) < 3 or numbers != sorted(set(numbers)):
        raise ValueError(f'Numbered sections need a unique increasing sequence for review: {volume.path}')
    changes, sections, modified_branches = {}, [], []
    for candidate in candidates:
        name = candidate['name']
        data = volume.files[name]
        identifier = candidate['id']
        if not identifier:
            occupied = {e.get('id') for e in ET.fromstring(data).iter() if e.get('id')}
            identifier = f'bokheim-section-{candidate["number"]}'
            while identifier in occupied:
                identifier += '-section'
            # Expat's byte offset points to the original opening tag. Locate its
            # closing bracket without confusing a '>' inside a quoted value.
            cursor, quoted = candidate['position'], None
            while cursor < len(data):
                char = data[cursor]
                if quoted is not None:
                    if char == quoted:
                        quoted = None
                elif char in (34, 39):
                    quoted = char
                elif char == 62:
                    break
                cursor += 1
            if cursor == len(data):
                raise ValueError(f'Unterminated section opening tag: {name}')
            changes.setdefault(name, []).append((cursor, (' id="' + identifier + '"').encode()))
        href = quote(volume.prefix + name, safe='/') + '#' + quote(identifier, safe='')
        reading_positions[(name, identifier)] = (candidate['spine_position'], candidate['position'])
        entry, parent = candidate['branch']
        # Keep the Aspect heading as an entry; sections are directly beneath
        # its Book, rather than hidden below one extra Aspect wrapper.
        aspect = re.match(r'^(?:First|Second) Aspect\.', entry['label'], re.I)
        destination = parent if aspect and parent is not None and 'Book.' in parent['label'] else entry
        if any(child['href'] == href for child in destination['children']):
            continue
        destination['children'].append({'label': f'§ {candidate["number"]}', 'href': href, 'children': []})
        if all(branch is not destination for branch in modified_branches):
            modified_branches.append(destination)
        sections.append({'number': candidate['number'], 'label': f'§ {candidate["number"]}', 'href': href,
                         'parent': destination['label'], 'anchor_added': candidate['id'] is None})
    def order(entry):
        if entry['href']:
            uri = urlsplit(entry['href'])
            location = reading_positions.get((unquote(uri.path).removeprefix(volume.prefix), unquote(uri.fragment)))
            if location is not None:
                return location
        return min((order(child) for child in entry['children']), default=(float('inf'), 0))
    for branch in modified_branches:
        branch['children'].sort(key=order)
    updated = {}
    for name, insertions in changes.items():
        data = volume.files[name]
        for offset, addition in sorted(insertions, reverse=True):
            data = data[:offset] + addition + data[offset:]
        # Adding an anchor must never alter text, markup structure, or links.
        before = ET.fromstring(volume.files[name])
        after = ET.fromstring(data)
        if ''.join(before.itertext()) != ''.join(after.itertext()):
            raise ValueError(f'Section anchors changed visible text: {name}')
        updated[name] = data
    return updated, sections


def make_nav(title, language, sections, pages):
    html = ET.Element(tag(XHTML, 'html'), {tag(XML, 'lang'): language, 'lang': language})
    head = ET.SubElement(html, tag(XHTML, 'head'))
    ET.SubElement(head, tag(XHTML, 'title')).text = title
    body = ET.SubElement(html, tag(XHTML, 'body'))
    ET.SubElement(body, tag(XHTML, 'h1')).text = title
    nav = ET.SubElement(body, tag(XHTML, 'nav'), {tag(EPUB, 'type'): 'toc', 'id': 'toc'})
    ET.SubElement(nav, tag(XHTML, 'h2')).text = 'Contents'
    add_list(nav, sections)
    if pages:
        nav = ET.SubElement(body, tag(XHTML, 'nav'), {tag(EPUB, 'type'): 'page-list', 'hidden': 'hidden'})
        ET.SubElement(nav, tag(XHTML, 'h2')).text = 'Pages'
        add_list(nav, pages)
    return xml_bytes(html)


def make_ncx(title, identifier, sections):
    ncx = ET.Element(tag(NCX, 'ncx'), {'version': '2005-1'})
    head = ET.SubElement(ncx, tag(NCX, 'head'))
    ET.SubElement(head, tag(NCX, 'meta'), {'name': 'dtb:uid', 'content': identifier})
    doc = ET.SubElement(ncx, tag(NCX, 'docTitle'))
    ET.SubElement(doc, tag(NCX, 'text')).text = title
    nav = ET.SubElement(ncx, tag(NCX, 'navMap'))
    counter = 0
    target_orders = {}

    def append(parent, entries):
        nonlocal counter
        for entry in entries:
            href = entry['href']
            children = entry['children']
            if href is None:
                append(parent, children)
                continue
            counter += 1
            order = target_orders.setdefault(href, len(target_orders) + 1)
            point = ET.SubElement(parent, tag(NCX, 'navPoint'), {'id': f'n{counter}', 'playOrder': str(order)})
            label = ET.SubElement(point, tag(NCX, 'navLabel'))
            ET.SubElement(label, tag(NCX, 'text')).text = entry['label']
            ET.SubElement(point, tag(NCX, 'content'), {'src': href})
            append(point, children)

    append(nav, sections)
    return xml_bytes(ncx)


def validate_epub(path):
    """Check reading order, resource links and fragment targets, including CSS."""
    with zipfile.ZipFile(path) as archive:
        infos = archive.infolist()
        if infos[0].filename != 'mimetype' or infos[0].compress_type != zipfile.ZIP_STORED:
            raise ValueError('mimetype must be first and uncompressed')
        if archive.testzip():
            raise ValueError('Archive CRC failure')
        files = {i.filename: archive.read(i.filename) for i in infos if not i.is_dir()}
    container = ET.fromstring(files['META-INF/container.xml'])
    package_path = container.find('.//' + tag(CONTAINER, 'rootfile')).get('full-path')
    package = ET.fromstring(files[package_path])
    manifest = package.find(tag(OPF, 'manifest'))
    items = {item.get('id'): item for item in manifest}
    if len(items) != len(manifest) or None in items:
        raise ValueError('Missing or duplicate manifest identifiers')
    package_ids = [element.get('id') for element in package.iter() if element.get('id')]
    if len(package_ids) != len(set(package_ids)):
        raise ValueError('Duplicate package identifiers')
    if sum('nav' in item.get('properties', '').split() for item in manifest) != 1:
        raise ValueError('The combined EPUB must have exactly one navigation document')
    documents = {}
    references = []
    resources = set()
    for item in manifest:
        resource = local_target(package_path, item.get('href'))
        if resource not in files:
            raise ValueError(f'Missing manifest resource: {resource}')
        resources.add(resource)
        for attr in ['fallback', 'media-overlay']:
            if item.get(attr) and item.get(attr) not in items:
                raise ValueError(f'Missing {attr} item: {item.get(attr)}')
        media_type = item.get('media-type')
        if media_type in ('application/xhtml+xml', 'image/svg+xml', 'application/x-dtbncx+xml', 'application/smil+xml'):
            doc = ET.fromstring(files[resource])
            documents[resource] = {e.get('id') for e in doc.iter() if e.get('id')}
            for element in doc.iter():
                if element.get(tag(XML, 'base')) is not None:
                    raise ValueError(f'Content xml:base is not supported: {resource}')
                for attr, value in element.attrib.items():
                    if attr.rsplit('}', 1)[-1] in ('href', 'src', 'poster', 'data'):
                        references.append((resource, value, True))
                    if attr == 'srcset':
                        raise ValueError(f'srcset resources require an explicit merge strategy: {resource}')
                    if attr == 'style':
                        references.extend((resource, a or b, False) for a, b in CSS_URL.findall(value))
                if element.tag == tag(XHTML, 'style') and element.text:
                    references.extend((resource, a or b, False) for a, b in CSS_URL.findall(element.text))
        elif media_type == 'text/css':
            references.extend((resource, a or b, False) for a, b in CSS_URL.findall(files[resource].decode('utf-8-sig')))
    for itemref in package.find(tag(OPF, 'spine')):
        if itemref.get('idref') not in items:
            raise ValueError(f'Missing spine item: {itemref.get("idref")}')
    checked = 0
    for origin, value, check_fragment in references:
        target = local_target(origin, value)
        if target is None:
            continue
        if target not in resources:
            raise ValueError(f'Missing or unmanifested local target: {origin}: {value}')
        fragment = unquote(urlsplit(value).fragment)
        if check_fragment and fragment and target in documents and fragment not in documents[target]:
            raise ValueError(f'Missing fragment target: {origin}: {value}')
        checked += 1
    return {'manifest_items': len(items), 'spine_items': len(package.find(tag(OPF, 'spine'))), 'local_references_checked': checked}


def validate_review(volumes, title, review):
    """An edition review applies only to these exact ordered source files."""
    if not isinstance(review, dict) or review.get('work') != title or not str(review.get('edition_evidence', '')).strip():
        raise ValueError('A reviewed merge needs its work title and edition evidence')
    actual = [{'sha256': v.sha256, 'creators': list(v.creators), 'contributors': list(v.contributors),
               'languages': list(v.languages)} for v in volumes]
    if review.get('sources') != actual:
        raise ValueError('Edition review does not match the ordered source hashes and credits')
    language_review = review.get('language_variation')
    if language_review is not None:
        languages = sorted({language for volume in volumes for language in volume.languages})
        if (not isinstance(language_review, dict) or not str(language_review.get('evidence', '')).strip()
                or language_review.get('languages') != languages):
            raise ValueError('Language review needs edition evidence and the exact source language union')
    reviewed = {v.sha256: v for v in volumes}
    creator_review = review.get('creator_variation')
    if creator_review is not None:
        creators = sorted({name for volume in volumes for name in volume.creators})
        if (not isinstance(creator_review, dict) or not str(creator_review.get('evidence', '')).strip()
                or creator_review.get('creators') != creators):
            raise ValueError('Creator review needs edition evidence and the exact source creator union')
    for credit in review.get('additional_credits', []):
        volume = reviewed.get(credit.get('source_sha256'))
        member = credit.get('member')
        if (volume is None or member not in volume.files or not str(credit.get('name', '')).strip()
                or credit.get('role') not in ('aut', 'ctb', 'edt', 'ill', 'trl')
                or not str(credit.get('evidence', '')).strip()):
            raise ValueError('Additional credit needs its exact source, name, role and evidence')
        source_text = ' '.join(''.join(ET.fromstring(volume.files[member]).itertext()).split())
        if ' '.join(credit['evidence'].split()) not in source_text:
            raise ValueError('Additional credit evidence is absent from its original source')
    for asset in review.get('additional_assets', []):
        volume = reviewed.get(asset.get('source_sha256'))
        member = asset.get('member', '')
        if (volume is None or member in volume.files or not member or member.startswith('/')
                or '..' in member.split('/') or asset.get('media_type') != 'image/png'
                or not str(asset.get('evidence', '')).strip()):
            raise ValueError('Additional asset must identify a missing source image and its evidence')
        data = Path(asset['path']).read_bytes()
        if hashlib.sha256(data).hexdigest() != asset.get('sha256') or not data.startswith(b'\x89PNG\r\n\x1a\n'):
            raise ValueError('Additional source image differs from its retained hash or format')
    navigation_seen = set()
    for repair in review.get('navigation_target_repairs', []):
        volume = reviewed.get(repair.get('source_sha256'))
        key = (repair.get('source_sha256'), repair.get('href'), repair.get('label'))
        if (volume is None or key in navigation_seen or not str(repair.get('reason', '')).strip()
                or not str(repair.get('original_heading', '')).strip()
                or not str(repair.get('target_href', '')).strip()
                or sum(n['href'] == repair.get('href') and n['label'] == repair.get('label')
                       for n in navigation_nodes(volume.toc)) != 1):
            raise ValueError('Navigation target repair needs a unique original entry and heading evidence')
        navigation_seen.add(key)
    for entry in review.get('added_navigation', []):
        volume = reviewed.get(entry.get('source_sha256'))
        key = (entry.get('source_sha256'), entry.get('member'), entry.get('target_id'))
        if (volume is None or entry.get('member') not in volume.files or key in navigation_seen
                or not all(str(entry.get(field, '')).strip()
                           for field in ('target_id', 'label', 'parent_href', 'parent_label', 'reason'))
                or sum(n['href'] == entry['parent_href'] and n['label'] == entry['parent_label']
                       for n in navigation_nodes(volume.toc)) != 1):
            raise ValueError('Added navigation needs a unique reviewed heading and original parent')
        navigation_seen.add(key)
    for entry in review.get('heading_navigation', []):
        volume = reviewed.get(entry.get('source_sha256'))
        key = (entry.get('source_sha256'), entry.get('member'), entry.get('target_id'))
        if (volume is None or entry.get('member') not in volume.files or key in navigation_seen
                or not str(entry.get('label', '')).strip() or not str(entry.get('target_id', '')).strip()
                or not str(entry.get('reason', '')).strip()):
            raise ValueError('Heading navigation needs a unique source heading and its reason')
        navigation_seen.add(key)
    for entry in review.get('flattened_navigation', []) + review.get('lifted_navigation', []):
        volume = reviewed.get(entry.get('source_sha256'))
        key = (entry.get('source_sha256'), entry.get('href'), entry.get('label'))
        if (volume is None or key in navigation_seen or not str(entry.get('reason', '')).strip()
                or sum(node['href'] == entry.get('href') and node['label'] == entry.get('label')
                       for node in navigation_nodes(volume.toc)) != 1):
            raise ValueError('Navigation review must identify a unique original entry and its reason')
        navigation_seen.add(key)
    seen = set()
    for repair in review.get('repairs', []):
        volume = reviewed.get(repair.get('source_sha256'))
        name = repair.get('member')
        if volume is None or name not in volume.files or (volume.sha256, name) in seen:
            raise ValueError('Repair must identify a unique resource in a reviewed source')
        manifest = volume.package.find(tag(OPF, 'manifest'))
        if not any(item.get('media-type') == 'application/xhtml+xml'
                   and local_target(volume.package_path, item.get('href')) == name for item in manifest):
            raise ValueError('Only reviewed XHTML resources can be repaired')
        if hashlib.sha256(volume.files[name]).hexdigest() != repair.get('member_sha256'):
            raise ValueError('Repair resource differs from the reviewed bytes')
        if not str(repair.get('reason', '')).strip() or not repair.get('replacements'):
            raise ValueError('Repair needs its reason and explicit replacements')
        seen.add((volume.sha256, name))


def preserve_metadata(destination, volume, prefix, names, seen_credits=None):
    source = volume.package.find(tag(OPF, 'metadata'))
    legacy = volume.package.get('version') == '2.0'
    ids = {e.get('id'): f'{prefix}metadata-{i}' for i, e in enumerate(source) if e.get('id')}
    roots = [e for e in source if e.tag in [tag(DC, name) for name in names]]
    if not roots:
        return
    # Keep the original all-roots-then-refinements order for ordinary merges,
    # preserving existing deterministic outputs and their revision identities.
    bundles = [roots] if seen_credits is None else [[root] for root in roots]
    for bundle in bundles:
        root = bundle[0]
        kept = list(bundle)
        retained_ids = {e.get('id') for e in bundle if e.get('id')}
        while True:
            refinements = [e for e in source if e not in kept and e.tag == tag(OPF, 'meta')
                           and e.get('refines', '').lstrip('#') in retained_ids]
            if not refinements:
                break
            kept.extend(refinements)
            retained_ids.update(e.get('id') for e in refinements if e.get('id'))
        if seen_credits is not None:
            # Ignore source-local IDs, preserving distinct roles/refinements.
            signature = (root.tag, root.text, tuple(sorted((k, v) for k, v in root.attrib.items() if k != 'id')),
                         tuple((e.text, tuple(sorted((k, v) for k, v in e.attrib.items() if k not in ('id', 'refines'))))
                               for e in kept[1:]))
            if signature in seen_credits:
                continue
            seen_credits.add(signature)
        for element in kept:
            copied = deepcopy(element)
            if copied.get('id'):
                copied.set('id', ids[copied.get('id')])
            if copied.get('refines'):
                copied.set('refines', '#' + ids[copied.get('refines').lstrip('#')])
            refinements = []
            if legacy and copied.tag in (tag(DC, 'creator'), tag(DC, 'contributor')):
                for attribute, property_name in [('role', 'role'), ('file-as', 'file-as')]:
                    value = copied.attrib.pop(tag(OPF, attribute), None)
                    if value is not None:
                        if not copied.get('id'):
                            copied.set('id', f'{prefix}metadata-{list(source).index(element)}')
                        attributes = {'refines': '#' + copied.get('id'), 'property': property_name}
                        if property_name == 'role':
                            attributes['scheme'] = 'marc:relators'
                        refinement = ET.Element(tag(OPF, 'meta'), attributes)
                        refinement.text = value
                        refinements.append(refinement)
            destination.append(copied)
            destination.extend(refinements)


def repair_resources(volume, review, updated):
    applied = []
    for repair in review.get('repairs', []) if review else []:
        if repair['source_sha256'] != volume.sha256:
            continue
        name = repair['member']
        before = updated.get(name, volume.files[name])
        after = before
        for replacement in repair['replacements']:
            old, new, count = replacement.get('old'), replacement.get('new'), replacement.get('count')
            if not isinstance(old, str) or not old or not isinstance(new, str) or type(count) is not int or count < 1:
                raise ValueError('Repair replacements need explicit nonempty old text, new text and positive count')
            if after.count(old.encode()) != count:
                raise ValueError(f'Repair replacement count differs from the reviewed source: {name}')
            after = after.replace(old.encode(), new.encode())
        before_xml, after_xml = ET.fromstring(before), ET.fromstring(after)
        text_preserved = ''.join(before_xml.itertext()) == ''.join(after_xml.itertext())
        restored_title = None
        if not text_preserved:
            original_titles = before_xml.findall('./' + tag(XHTML, 'head') + '/' + tag(XHTML, 'title'))
            repaired_titles = after_xml.findall('./' + tag(XHTML, 'head') + '/' + tag(XHTML, 'title'))
            if (repair.get('restore_empty_document_title') is not True
                    or len(original_titles) != 1 or len(repaired_titles) != 1
                    or (original_titles[0].text or '').strip() or len(original_titles[0])
                    or len(repaired_titles[0]) or repaired_titles[0].text != volume.title):
                raise ValueError(f'Repair changed source text: {name}')
            # Only a missing HTML head title may borrow the exact retained
            # package title. Reverting that one node must recover all original
            # text, including whitespace; body text remains unchanged.
            restored_title = repaired_titles[0].text
            repaired_titles[0].text = original_titles[0].text
            if ''.join(before_xml.itertext()) != ''.join(after_xml.itertext()):
                raise ValueError(f'Repair changed source text: {name}')
        updated[name] = after
        applied.append({**deepcopy(repair), 'before_sha256': hashlib.sha256(before).hexdigest(),
                        'after_sha256': hashlib.sha256(after).hexdigest(), 'text_preserved': text_preserved,
                        'reading_text_preserved': True, 'restored_document_title': restored_title})
    return applied


def combine_descriptions(items):
    """Keep distinct source text, attributing it to its original component."""
    descriptions = {}
    for label, description in items:
        if not isinstance(description, str) or not description.strip():
            continue
        description = description.strip()
        labels = descriptions.setdefault(description, [])
        if label and label not in labels:
            labels.append(label)
    if len(descriptions) == 1:
        return next(iter(descriptions))
    return '\n\n'.join(('; '.join(labels) + '\n' if labels else '') + text for text, labels in descriptions.items())


def navigation_nodes(entries):
    for entry in entries:
        yield entry
        yield from navigation_nodes(entry['children'])


def repair_navigation_targets(volume, entries, review, updated):
    result = deepcopy(entries)
    for repair in review.get('navigation_target_repairs', []) if review else []:
        if repair['source_sha256'] != volume.sha256:
            continue
        href = repair['target_href']
        target = local_target('nav.xhtml', href)
        if target is None or not target.startswith(volume.prefix):
            raise ValueError('Navigation repair target must be inside its original volume')
        member = target[len(volume.prefix):]
        fragment = unquote(urlsplit(href).fragment)
        data = updated.get(member, volume.files.get(member))
        if data is None or not fragment:
            raise ValueError('Navigation repair needs an existing source heading anchor')
        anchors = [e for e in ET.fromstring(data).iter() if e.get('id') == fragment]
        if (len(anchors) != 1 or anchors[0].tag not in {tag(XHTML, 'h' + str(i)) for i in range(1, 7)}
                or ' '.join(''.join(anchors[0].itertext()).split()) != repair['original_heading']):
            raise ValueError('Navigation repair target must retain exact original heading text')
        entry = next(n for n in navigation_nodes(result)
                     if n['href'] == repair['href'] and n['label'] == repair['label'])
        entry['href'] = href
    return result


def flatten_navigation(entries, selected, lifted=()):
    result = []
    for entry in entries:
        children = flatten_navigation(entry['children'], selected, lifted)
        key = (entry['href'], entry['label'])
        if key in selected:
            result.extend(children)
        elif key in lifted:
            result.append({**entry, 'children': []})
            result.extend(children)
        else:
            result.append({**entry, 'children': children})
    return result


def reviewed_heading_navigation(volume, review, updated):
    """Reuse exact source heading text when the supplied TOC contains pages only."""
    selected = [e for e in review.get('heading_navigation', []) if e['source_sha256'] == volume.sha256] if review else []
    if not selected:
        return None
    manifest = volume.package.find(tag(OPF, 'manifest'))
    items = {item.get('id'): item for item in manifest}
    reading_order = [local_target(volume.package_path, items[ref.get('idref')].get('href'))
                     for ref in volume.package.find(tag(OPF, 'spine'))]
    documents, positions, result = {}, [], []
    for entry in selected:
        member = entry['member']
        if member not in reading_order or not any(
                item.get('media-type') == 'application/xhtml+xml'
                and local_target(volume.package_path, item.get('href')) == member for item in manifest):
            raise ValueError('Heading navigation must target original reading-order XHTML')
        if member not in documents:
            document = ET.fromstring(updated.get(member, volume.files[member]))
            body = document.find(tag(XHTML, 'body'))
            documents[member] = list(body.iter()) if body is not None else []
        matches = [(index, node) for index, node in enumerate(documents[member])
                   if node.get('id') == entry['target_id']]
        if len(matches) != 1:
            raise ValueError('Heading navigation needs a unique heading anchor')
        index, node = matches[0]
        heading_element = node.tag in {tag(XHTML, name) for name in ('p', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6')}
        # Some Gutenberg transcriptions mark their book headings as plain
        # divs. Accept their exact text without changing the source markup;
        # a container with descendant elements is not a heading paragraph.
        plain_div = node.tag == tag(XHTML, 'div') and len(node) == 0
        if (not (heading_element or plain_div)
                or ' '.join(''.join(node.itertext()).split()) != entry['label']):
            raise ValueError('Heading navigation label must exactly retain source heading text')
        positions.append((reading_order.index(member), index))
        result.append({'label': entry['label'], 'href': volume.prefix + member + '#' + entry['target_id'], 'children': []})
    if positions != sorted(positions):
        raise ValueError('Heading navigation must follow original reading order')
    return result


def augment_navigation(volume, entries, review, updated):
    """Add omitted source headings under their reviewed original TOC parent."""
    additions = [e for e in review.get('added_navigation', [])
                 if e['source_sha256'] == volume.sha256] if review else []
    if not additions:
        return entries
    manifest = {item.get('id'): item for item in volume.package.find(tag(OPF, 'manifest'))}
    reading = [local_target(volume.package_path, manifest[ref.get('idref')].get('href'))
               for ref in volume.package.find(tag(OPF, 'spine'))]
    documents = {}

    def document(member):
        if member not in reading:
            raise ValueError('Added navigation must target original reading-order XHTML')
        if member not in documents:
            body = ET.fromstring(updated.get(member, volume.files[member])).find(tag(XHTML, 'body'))
            nodes = list(body.iter()) if body is not None else []
            parents = {child: parent for parent in nodes for child in parent}
            documents[member] = nodes, parents
        return documents[member]

    def position(href):
        member, _, fragment = href[len(volume.prefix):].partition('#')
        member, fragment = unquote(member), unquote(fragment)
        nodes, _ = document(member)
        matches = [i for i, n in enumerate(nodes) if n.get('id') == fragment] if fragment else [0]
        if len(matches) != 1:
            raise ValueError('Added navigation needs a unique source anchor')
        return reading.index(member), matches[0]

    result = deepcopy(entries)
    existing = {n['href'] for n in navigation_nodes(result)}
    headings = {tag(XHTML, f'h{i}') for i in range(1, 7)}
    changed = []
    for addition in additions:
        member, fragment = addition['member'], addition['target_id']
        nodes, parents = document(member)
        matches = [n for n in nodes if n.get('id') == fragment]
        if len(matches) != 1:
            raise ValueError('Added navigation needs a unique source anchor')
        heading = matches[0]
        if heading.tag == tag(XHTML, 'a') and parents.get(heading) is not None:
            heading = parents[heading]
        if addition.get('adjacent_heading'):
            # Some sources put the section number and its real title in
            # consecutive headings, with the anchor on the number only.
            label = ' '.join(''.join(heading.itertext()).split())
            parent_element = parents.get(heading)
            if (heading.tag not in headings or parent_element is None
                    or not re.fullmatch(r'(?:SECTION|SECT|CHAPTER|BOOK|PART)[ .]+(?:\d+|[IVXLCDM]+)(?:_[A-Za-z0-9]+)?\.?', label, re.I)
                    or (heading.tail or '').strip()):
                raise ValueError('Adjacent heading title needs an original numbered heading anchor')
            siblings = list(parent_element)
            following = siblings.index(heading) + 1
            if following >= len(siblings):
                raise ValueError('Adjacent heading title is absent')
            heading = siblings[following]
        plain_heading = (addition.get('plain_heading') is True
                         and (heading.tag == tag(XHTML, 'p')
                              or (heading.tag == tag(XHTML, 'div') and len(heading) == 0)))
        inline_heading = (addition.get('inline_heading') is True
                          and heading.tag in {tag(XHTML, name) for name in ('span', 'i', 'b', 'em', 'strong')})
        if (heading.tag not in headings and not plain_heading and not inline_heading
                or ' '.join(''.join(heading.itertext()).split()) != addition['label']):
            raise ValueError('Added navigation must retain exact original heading text')
        href = volume.prefix + quote(member, safe='/') + '#' + quote(fragment, safe='')
        if href in existing:
            raise ValueError('Added navigation repeats an existing TOC target')
        parent = next(n for n in navigation_nodes(result)
                      if n['href'] == addition['parent_href'] and n['label'] == addition['parent_label'])
        parent['children'].append({'label': addition['label'], 'href': href, 'children': []})
        existing.add(href)
        changed.append(parent)
    for parent in changed:
        parent['children'].sort(key=lambda n: position(n['href']))
    return result


def merge_epubs(paths, output, title, labels=None, modified=None, expand_sections=False, review=None, description=None):
    if len(paths) < 2:
        raise ValueError('At least two explicitly ordered volumes are required')
    output = Path(output)
    if output.exists() or output.resolve() in {Path(p).resolve() for p in paths}:
        raise ValueError('Output must be a new file distinct from every source volume')
    if not title.strip():
        raise ValueError('The combined edition needs a title')
    if labels is not None and len(labels) != len(paths):
        raise ValueError('Supply one --label per volume, in reading order')
    labels = labels or [f'Volume {i}' for i in range(1, len(paths) + 1)]
    volumes = [read_volume(path, label, i) for i, (path, label) in enumerate(zip(paths, labels), 1)]
    if len({volume.sha256 for volume in volumes}) != len(volumes):
        raise ValueError('The same volume cannot be included more than once')
    if review is not None:
        validate_review(volumes, title, review)
    different_contributors = any(volume.contributors != volumes[0].contributors for volume in volumes)
    different_creators = any(volume.creators != volumes[0].creators for volume in volumes)
    reviewed_creators = review is not None and review.get('creator_variation') is not None
    different_languages = any(volume.languages != volumes[0].languages for volume in volumes)
    reviewed_languages = review is not None and review.get('language_variation') is not None
    for volume in volumes:
        if (not (volume.creators or volume.contributors) or not volume.languages
                or (different_creators and not reviewed_creators)
                or (different_contributors and review is None)
                or (different_languages and not reviewed_languages)):
            raise ValueError('All volumes must have the same credited authors and languages, and the same translators/contributors')
    if (different_creators or different_contributors or different_languages) and len({v.package.get('prefix') for v in volumes}) != 1:
        raise ValueError('Reviewed contributor variation requires matching metadata vocabularies')
    description_source = 'catalog_components' if description is not None else 'source_packages'
    if description is None:
        description = combine_descriptions((v.label, e.text) for v in volumes
            for e in v.package.find(tag(OPF, 'metadata')).findall(tag(DC, 'description')))
    if not isinstance(description, str):
        raise ValueError('Retained description must be text')
    modified = first_source_modified(volumes) if modified is None else modified
    if not valid_modified(modified):
        raise ValueError('Last-modified timestamp must be a valid UTC date in YYYY-MM-DDThh:mm:ssZ format')
    identity = [title, [v.sha256 for v in volumes], modified]
    if expand_sections:
        identity.append('numbered-sections-v1')
    if review is not None:
        identity.append(['reviewed-merge-v1', review])
    if description:
        identity.append(['retained-description-v1', description])
    identifier = 'urn:uuid:' + str(uuid.uuid5(uuid.NAMESPACE_URL, json.dumps(identity, sort_keys=True)))
    package = ET.Element(tag(OPF, 'package'), {'version': '3.0', 'unique-identifier': 'merged-id'})
    metadata = ET.SubElement(package, tag(OPF, 'metadata'))
    ET.SubElement(metadata, tag(DC, 'identifier'), {'id': 'merged-id'}).text = identifier
    ET.SubElement(metadata, tag(DC, 'title')).text = title
    if description:
        ET.SubElement(metadata, tag(DC, 'description')).text = description
    ET.SubElement(metadata, tag(OPF, 'meta'), {'property': 'dcterms:modified'}).text = modified
    if volumes[0].package.get('prefix'):
        package.set('prefix', volumes[0].package.get('prefix'))
    if different_creators or different_contributors:
        seen_credits = set()
        for index, volume in enumerate(volumes, 1):
            preserve_metadata(metadata, volume, f'v{index:03}-', ['creator', 'contributor'], seen_credits)
        preserve_metadata(metadata, volumes[0], '', ['rights'] if different_languages else ['language', 'rights'])
    else:
        names = ['creator', 'contributor', 'rights'] if different_languages else ['creator', 'contributor', 'language', 'rights']
        preserve_metadata(metadata, volumes[0], '', names)
    if different_languages:
        seen_languages = set()
        for index, volume in enumerate(volumes, 1):
            preserve_metadata(metadata, volume, f'v{index:03}-', ['language'], seen_languages)
    extra_credits = set()
    for credit in review.get('additional_credits', []) if review else []:
        key = (credit['name'], credit['role'])
        if key in extra_credits:
            continue
        extra_credits.add(key)
        identity = f'reviewed-credit-{len(extra_credits)}'
        ET.SubElement(metadata, tag(DC, 'creator' if credit['role'] == 'aut' else 'contributor'),
                      {'id': identity}).text = credit['name']
        ET.SubElement(metadata, tag(OPF, 'meta'),
                      {'refines': '#' + identity, 'property': 'role', 'scheme': 'marc:relators'}).text = credit['role']
    seen = set()
    for volume in volumes:
        for element in volume.package.find(tag(OPF, 'metadata')):
            if element.tag not in (tag(DC, 'subject'), tag(DC, 'source'), tag(DC, 'identifier')) or not element.text:
                continue
            destination = tag(DC, 'source') if element.tag == tag(DC, 'identifier') else element.tag
            key = (destination, element.text)
            if key not in seen:
                ET.SubElement(metadata, destination).text = element.text
                seen.add(key)
    manifest = ET.SubElement(package, tag(OPF, 'manifest'))
    ET.SubElement(manifest, tag(OPF, 'item'), {'id': 'merged-nav', 'href': 'nav.xhtml', 'media-type': 'application/xhtml+xml', 'properties': 'nav'})
    ET.SubElement(manifest, tag(OPF, 'item'), {'id': 'merged-ncx', 'href': 'toc.ncx', 'media-type': 'application/x-dtbncx+xml'})
    spine = ET.SubElement(package, tag(OPF, 'spine'), {'toc': 'merged-ncx'})
    ET.SubElement(spine, tag(OPF, 'itemref'), {'idref': 'merged-nav'})
    entries, pages, resources, provenance, modified_resources = [], [], {}, [], set()
    anchor_resources, repaired_resources = set(), set()
    for index, volume in enumerate(volumes, 1):
        updated_content, added_sections = expand_numbered_sections(volume) if expand_sections else ({}, [])
        anchor_resources.update(volume.prefix + name for name in updated_content)
        applied_repairs = repair_resources(volume, review, updated_content)
        repaired_resources.update(volume.prefix + repair['member'] for repair in applied_repairs)
        source_manifest = volume.package.find(tag(OPF, 'manifest'))
        legacy_cover = next((e.get('content') for e in volume.package.find(tag(OPF, 'metadata'))
                             if e.tag == tag(OPF, 'meta') and e.get('name') == 'cover'), None)
        obsolete = {local_target(volume.package_path, item.get('href')) for item in source_manifest if item.get('media-type') == 'application/x-dtbncx+xml'}
        retained = [item for item in source_manifest if item.get('media-type') != 'application/x-dtbncx+xml']
        id_map = {item.get('id'): f'v{index:03}_{item.get("id")}' for item in retained}
        for item in retained:
            merged = deepcopy(item)
            merged.set('id', id_map[item.get('id')])
            merged.set('href', relocated_uri(volume.package_path, item.get('href'), volume.prefix))
            properties = [p for p in item.get('properties', '').split() if p != 'nav' and (index == 1 or p != 'cover-image')]
            if (volume.package.get('version') == '2.0' and index == 1
                    and item.get('id') == legacy_cover and 'cover-image' not in properties):
                properties.append('cover-image')
            merged.attrib.pop('properties', None)
            if properties:
                merged.set('properties', ' '.join(properties))
            for attr in ['fallback', 'media-overlay']:
                if item.get(attr):
                    merged.set(attr, id_map[item.get(attr)])
            manifest.append(merged)
        source_spine = volume.package.find(tag(OPF, 'spine'))
        for asset_index, asset in enumerate(review.get('additional_assets', []) if review else [], 1):
            if asset['source_sha256'] != volume.sha256:
                continue
            destination = volume.prefix + asset['member']
            if destination in resources:
                raise ValueError('Additional source image would replace an existing resource')
            resources[destination] = Path(asset['path']).read_bytes()
            image_id = f'v{index:03}_reviewed_asset_{asset_index}'
            document_id = image_id + '_document'
            document = destination + '.xhtml'
            if document in resources or asset['member'] + '.xhtml' in volume.files:
                raise ValueError('Source image fallback would replace an existing resource')
            html = ET.Element(tag(XHTML, 'html'), {'lang': volume.languages[0]})
            head = ET.SubElement(html, tag(XHTML, 'head'))
            ET.SubElement(head, tag(XHTML, 'title')).text = volume.title
            body = ET.SubElement(html, tag(XHTML, 'body'))
            ET.SubElement(body, tag(XHTML, 'img'), {'src': quote(posixpath.basename(destination), safe=''), 'alt': ''})
            resources[document] = xml_bytes(html)
            ET.SubElement(manifest, tag(OPF, 'item'), {'id': image_id, 'fallback': document_id,
                          'href': quote(destination, safe='/'), 'media-type': asset['media_type']})
            ET.SubElement(manifest, tag(OPF, 'item'), {'id': document_id,
                          'href': quote(document, safe='/'), 'media-type': 'application/xhtml+xml'})
            ET.SubElement(spine, tag(OPF, 'itemref'), {'idref': document_id, 'linear': 'no'})
        first_href = None
        for itemref in source_spine:
            merged = deepcopy(itemref)
            merged.set('idref', id_map[itemref.get('idref')])
            if merged.get('id'):
                merged.set('id', f'v{index:03}_{merged.get("id")}')
            spine.append(merged)
            if first_href is None and itemref.get('linear', 'yes') != 'no':
                source_item = next(item for item in retained if item.get('id') == itemref.get('idref'))
                first_href = relocated_uri(volume.package_path, source_item.get('href'), volume.prefix)
        if first_href is None:
            raise ValueError(f'Volume has no linear reading content: {volume.path}')
        flattened = {(entry['href'], entry['label']) for entry in review.get('flattened_navigation', [])
                     if entry['source_sha256'] == volume.sha256} if review else set()
        lifted = {(entry['href'], entry['label']) for entry in review.get('lifted_navigation', [])
                  if entry['source_sha256'] == volume.sha256} if review else set()
        heading_entries = reviewed_heading_navigation(volume, review, updated_content)
        # Reviewed wrappers can occur beneath titlepage or editor headings.
        # Retain chapter hierarchy and labels while promoting only those nodes.
        navigation = augment_navigation(volume, heading_entries if heading_entries is not None else volume.toc,
                                        review, updated_content)
        navigation = repair_navigation_targets(volume, navigation, review, updated_content)
        entries.extend(flatten_navigation(navigation, flattened, lifted))
        for page in volume.pages:
            copied = deepcopy(page)
            copied['label'] = volume.label + ': ' + copied['label']
            pages.append(copied)
        for name, data in volume.files.items():
            if name != 'mimetype' and not name.startswith('META-INF/') and name != volume.package_path and name not in obsolete:
                resources[volume.prefix + name] = updated_content.get(name, data)
                if name in updated_content:
                    modified_resources.add(volume.prefix + name)
        provenance.append({'label': volume.label, 'title': volume.title, 'path': str(volume.path.resolve()),
                           'sha256': volume.sha256, 'spine_items': len(source_spine),
                           'added_numbered_sections': added_sections,
                           'applied_repairs': applied_repairs,
                           'retained_content_files': sum(name.startswith(volume.prefix) for name in resources)})
        if volume.package.get('version') == '2.0':
            provenance[-1]['input_format'] = 'EPUB2'
            provenance[-1]['navigation_source'] = 'original_ncx'
            provenance[-1]['source_timestamp'] = legacy_timestamp(volume.package)
    container = ET.Element(tag(CONTAINER, 'container'), {'version': '1.0'})
    roots = ET.SubElement(container, tag(CONTAINER, 'rootfiles'))
    ET.SubElement(roots, tag(CONTAINER, 'rootfile'), {'full-path': 'content.opf', 'media-type': 'application/oebps-package+xml'})
    resources.update({'META-INF/container.xml': xml_bytes(container), 'content.opf': xml_bytes(package),
                      'nav.xhtml': make_nav(title, volumes[0].languages[0], entries, pages), 'toc.ncx': make_ncx(title, identifier, entries)})
    output.parent.mkdir(parents=True, exist_ok=True)
    descriptor, name = tempfile.mkstemp(prefix=output.name + '.', suffix='.partial', dir=output.parent)
    os.close(descriptor)
    temporary = Path(name)
    try:
        with zipfile.ZipFile(temporary, 'w', compression=zipfile.ZIP_DEFLATED) as archive:
            def write(name, data, compress=zipfile.ZIP_DEFLATED):
                info = zipfile.ZipInfo(name, (1980, 1, 1, 0, 0, 0))
                info.compress_type = compress
                archive.writestr(info, data)
            write('mimetype', b'application/epub+zip', zipfile.ZIP_STORED)
            for name, data in resources.items():
                write(name, data)
        verification = validate_epub(temporary)
        # Unchanged resources are byte-identical; modified content differs only
        # by the explicit anchors verified during section expansion.
        with zipfile.ZipFile(temporary) as archive:
            for volume in volumes:
                for name, data in volume.files.items():
                    destination = volume.prefix + name
                    expected = resources.get(destination) if destination in modified_resources else data
                    if destination in resources and archive.read(destination) != expected:
                        raise ValueError(f'Content changed during merge: {destination}')
        # Hard-link publication refuses to replace an existing file, even when
        # another process creates the requested output during validation.
        os.link(temporary, output)
    finally:
        temporary.unlink(missing_ok=True)
    return {'title': title, 'identifier': identifier, 'modified': modified, 'output': str(output.resolve()),
            'sha256': hashlib.sha256(output.read_bytes()).hexdigest(), 'bytes': output.stat().st_size,
            'volumes': provenance, 'validation': verification, 'content_preserved': not modified_resources,
            'visible_text_preserved': True, 'anchor_only_modified_resources': sorted(anchor_resources - repaired_resources),
            'repaired_resources': sorted(repaired_resources), 'review': deepcopy(review),
            'description': description, 'description_source': description_source,
            'credit_policy': 'reviewed_contributor_variation' if different_contributors else 'matching', 'toc_volume_headings': False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('volumes', nargs='+', type=Path, help='Source EPUB2/3 files in reading order; output is EPUB3')
    parser.add_argument('--output', type=Path, required=True, help='New EPUB filename without volume information')
    parser.add_argument('--title', required=True, help='Original work title without volume information')
    parser.add_argument('--label', action='append', help='One navigation heading per volume')
    parser.add_argument('--modified', help='Override the first valid source last-modified timestamp')
    parser.add_argument('--report', type=Path, help='Write provenance and validation JSON alongside the EPUB')
    parser.add_argument('--expand-numbered-sections', action='store_true', help='Add explicit § paragraphs to the TOC, inserting missing anchors only')
    parser.add_argument('--review', type=Path, help='Explicit edition review with exact source hashes, credits and optional text-preserving XHTML repairs')
    parser.add_argument('--description', help='Retained description from the catalog/source pages; otherwise retain source package descriptions')
    args = parser.parse_args()
    try:
        review = json.loads(args.review.read_text()) if args.review else None
        report = merge_epubs(args.volumes, args.output, args.title, args.label, args.modified, args.expand_numbered_sections, review, args.description)
    except (ValueError, KeyError, OSError, ET.ParseError, zipfile.BadZipFile) as error:
        parser.exit(1, f'Merge failed: {error}\n')
    if args.report:
        args.report.write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
    print(json.dumps(report, ensure_ascii=False))


if __name__ == '__main__':
    main()
