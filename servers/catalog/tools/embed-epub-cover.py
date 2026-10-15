#!/usr/bin/env python3
"""Write a selected catalog cover into a new EPUB, preserving its other files.

The caller must compute the new content hash and update catalog references;
never overwrite an EPUB at its old content-addressed filename.
"""
from copy import copy
import hashlib
import io
from pathlib import Path
import posixpath
import re
from urllib.parse import unquote
import xml.etree.ElementTree as ET
import zipfile

from PIL import Image

OPF = 'http://www.idpf.org/2007/opf'
CONTAINER = 'urn:oasis:names:tc:opendocument:xmlns:container'
XHTML = 'http://www.w3.org/1999/xhtml'


def resolve(package, href):
    path = posixpath.normpath(posixpath.join(posixpath.dirname(package), unquote(href.split('#', 1)[0])))
    if path.startswith('../') or path.startswith('/'):
        raise ValueError('EPUB resource escapes the archive')
    return path


def cover_item(package):
    manifest = package.find(f'{{{OPF}}}manifest')
    metadata = package.find(f'{{{OPF}}}metadata')
    if manifest is None or metadata is None:
        raise ValueError('EPUB package has no manifest or metadata')
    items = list(manifest)
    properties = [item for item in items if 'cover-image' in item.get('properties', '').split()]
    if properties:
        return properties[0]
    for meta in metadata:
        if meta.get('name', '').lower() == 'cover':
            found = next((item for item in items if item.get('id') == meta.get('content')), None)
            if found is not None:
                return found
    return None


def serialize_package(root, original):
    namespaces = list(ET.iterparse(io.BytesIO(original), events=['start-ns']))
    for _, (prefix, uri) in namespaces:
        if not re.fullmatch(r'ns\d+', prefix):
            ET.register_namespace(prefix, uri)
    data = ET.tostring(root, encoding='utf-8', xml_declaration=True)
    # QName-valued metadata can use a declared prefix without any element in
    # that namespace. ElementTree otherwise drops those declarations.
    start = data.find(b'<', data.find(b'?>') + 2)
    end = data.find(b'>', start)
    opening = data[start:end]
    additions = []
    for _, (prefix, uri) in namespaces:
        if not prefix: continue
        token = f'xmlns:{prefix}='.encode()
        if token not in opening and not any(token in item for item in additions):
            escaped = uri.replace('&', '&amp;').replace('"', '&quot;')
            additions.append(f' xmlns:{prefix}="{escaped}"'.encode())
    return data[:end] + b''.join(additions) + data[end:]


def rewrite(source, destination, cover):
    source, destination, cover = Path(source), Path(destination), Path(cover)
    if source.resolve() == destination.resolve():
        raise ValueError('Write a derivative EPUB; preserve the original')
    with Image.open(cover) as picture:
        picture.load(); image = picture.convert('RGB')
        width, height = image.size
        original_format = picture.format
    replacements = {}; additions = {}
    with zipfile.ZipFile(source) as original:
        if len(set(original.namelist())) != len(original.namelist()):
            raise ValueError('EPUB has duplicate ZIP member names')
        if original.read('mimetype') != b'application/epub+zip':
            raise ValueError('Not an EPUB archive')
        container = ET.fromstring(original.read('META-INF/container.xml'))
        rootfile = container.find(f'.//{{{CONTAINER}}}rootfile')
        if rootfile is None: raise ValueError('No EPUB rootfile')
        package_path = rootfile.attrib['full-path']
        package_bytes = original.read(package_path)
        package = ET.fromstring(package_bytes)
        selected = cover_item(package)
        if selected is not None and selected.get('media-type') in {'image/jpeg', 'image/png', 'image/gif', 'image/webp'}:
            target = resolve(package_path, selected.attrib['href'])
            if target not in original.namelist(): raise ValueError('Declared cover is missing')
            output = io.BytesIO()
            fmt = {'image/jpeg': 'JPEG', 'image/png': 'PNG', 'image/gif': 'GIF', 'image/webp': 'WEBP'}[selected.attrib['media-type']]
            if fmt == 'JPEG' and original_format == 'JPEG':
                output.write(cover.read_bytes())
            elif fmt == 'JPEG':
                image.save(output, fmt, quality=95, subsampling=0)
            elif fmt == 'WEBP':
                image.save(output, fmt, lossless=True)
            else:
                image.save(output, fmt)
            replacements[target] = output.getvalue()
            embedded = target
        else:
            manifest = package.find(f'{{{OPF}}}manifest'); metadata = package.find(f'{{{OPF}}}metadata'); spine = package.find(f'{{{OPF}}}spine')
            if spine is None: raise ValueError('EPUB has no spine')
            suffix = hashlib.sha256(cover.read_bytes()).hexdigest()[:16]
            name = f'bokheim-cover-{suffix}.jpg'; page_name = f'bokheim-cover-{suffix}.xhtml'
            image_id = f'bokheim-cover-{suffix}'; page_id = f'bokheim-cover-page-{suffix}'
            embedded = resolve(package_path, name); page_path = resolve(package_path, page_name)
            if embedded in original.namelist() or page_path in original.namelist():
                raise ValueError('Generated cover resources already exist')
            old_guide = package.find(f'{{{OPF}}}guide')
            old_cover_pages = {resolve(package_path, reference.get('href', '')) for reference in old_guide or [] if reference.get('type') == 'cover'}
            cover_page_ids = {item.get('id') for item in manifest if resolve(package_path, item.get('href', '')) in old_cover_pages}
            if selected is not None and selected.get('media-type') == 'image/svg+xml':
                old_svg = resolve(package_path, selected.attrib['href'])
                if old_svg in original.namelist():
                    href = posixpath.relpath(embedded, posixpath.dirname(old_svg) or '.')
                    replacements[old_svg] = (f'<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{width}" height="{height}" viewBox="0 0 {width} {height}"><image width="{width}" height="{height}" xlink:href="{href}"/></svg>').encode()
                    cover_page_ids.add(selected.get('id'))
            if selected is not None and selected.get('media-type') == 'application/xhtml+xml':
                old_cover_pages.add(resolve(package_path, selected.attrib['href']))
                cover_page_ids.add(selected.get('id'))
            for path in old_cover_pages:
                try:
                    leaf = ET.fromstring(original.read(path))
                except (ET.ParseError, KeyError):
                    continue
                body = leaf.find(f'{{{XHTML}}}body')
                if leaf.tag != f'{{{XHTML}}}html' or body is None or ''.join(body.itertext()).strip():
                    continue
                href = posixpath.relpath(embedded, posixpath.dirname(path) or '.')
                replacements[path] = (f'<html xmlns="{XHTML}"><head><title>Cover</title></head><body style="margin:0;text-align:center"><img src="{href}" alt="Cover" style="max-width:100%;height:auto"/></body></html>').encode()
            # Replace image-only cover leaves. Even a short passage must stay
            # in the reading order if a source has labelled it as the cover.
            for reference in list(spine):
                if reference.get('idref') not in cover_page_ids: continue
                item = next((item for item in manifest if item.get('id') == reference.get('idref')), None)
                if item is None: continue
                path = resolve(package_path, item.get('href', ''))
                if path in replacements: spine.remove(reference)
            for item in manifest:
                flags = item.get('properties', '').split()
                if 'cover-image' in flags:
                    flags.remove('cover-image')
                    if flags: item.set('properties', ' '.join(flags))
                    else: item.attrib.pop('properties', None)
            attributes = {'id': image_id, 'href': name, 'media-type': 'image/jpeg'}
            if package.get('version', '').startswith('3'): attributes['properties'] = 'cover-image'
            ET.SubElement(manifest, f'{{{OPF}}}item', attributes)
            ET.SubElement(manifest, f'{{{OPF}}}item', {'id': page_id, 'href': page_name, 'media-type': 'application/xhtml+xml'})
            old_meta = [meta for meta in metadata if meta.get('name', '').lower() == 'cover']
            for meta in old_meta: metadata.remove(meta)
            ET.SubElement(metadata, f'{{{OPF}}}meta', {'name': 'cover', 'content': image_id})
            spine.insert(0, ET.Element(f'{{{OPF}}}itemref', {'idref': page_id, 'linear': 'yes'}))
            guide = package.find(f'{{{OPF}}}guide')
            if guide is None: guide = ET.SubElement(package, f'{{{OPF}}}guide')
            for reference in list(guide):
                if reference.get('type') == 'cover': guide.remove(reference)
            ET.SubElement(guide, f'{{{OPF}}}reference', {'type': 'cover', 'title': 'Cover', 'href': page_name})
            output = io.BytesIO(); image.save(output, 'JPEG', quality=95, subsampling=0)
            additions[embedded] = output.getvalue()
            additions[page_path] = (f'<?xml version="1.0" encoding="utf-8"?><html xmlns="{XHTML}"><head><title>Cover</title><meta name="viewport" content="width={width},height={height}"/></head><body style="margin:0;text-align:center"><img src="{name}" alt="Cover" style="max-width:100%;height:auto"/></body></html>').encode()
            replacements[package_path] = serialize_package(package, package_bytes)
        destination.parent.mkdir(parents=True, exist_ok=True)
        temporary = destination.with_name(destination.name + '.partial')
        try:
            with zipfile.ZipFile(temporary, 'w') as updated:
                source_sha = hashlib.sha256(source.read_bytes()).hexdigest()
                comment = original.comment + b'\nBokheim selected cover; original SHA-256: ' + source_sha.encode()
                if len(comment) > 65535: raise ValueError('EPUB ZIP comment has no room for cover provenance')
                updated.comment = comment
                # EPUB requires the uncompressed mimetype to be the first entry.
                entries = sorted(original.infolist(), key=lambda entry: entry.filename != 'mimetype')
                for entry in entries:
                    info = copy(entry)
                    if entry.filename == 'mimetype': info.compress_type = zipfile.ZIP_STORED
                    updated.writestr(info, replacements.get(entry.filename, original.read(entry.filename)))
                for path, content in additions.items():
                    updated.writestr(path, content, compress_type=zipfile.ZIP_DEFLATED)
            with zipfile.ZipFile(temporary) as updated:
                assert updated.testzip() is None
                assert updated.infolist()[0].filename == 'mimetype' and updated.infolist()[0].compress_type == zipfile.ZIP_STORED
                for name in original.namelist():
                    if name not in replacements: assert updated.read(name) == original.read(name), name
                with Image.open(io.BytesIO(updated.read(embedded))) as check:
                    check.load(); assert check.size == (width, height)
            temporary.replace(destination)
        except Exception:
            temporary.unlink(missing_ok=True)
            raise
    return {'source': str(source), 'output': str(destination), 'embedded_cover_path': embedded, 'cover_size': [width,height], 'changed_entries': sorted(replacements), 'added_entries': sorted(additions), 'non_cover_entries_preserved': True, 'sha256': hashlib.sha256(destination.read_bytes()).hexdigest()}


if __name__ == '__main__':
    import argparse
    import json

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('destination', type=Path)
    parser.add_argument('cover', type=Path)
    args = parser.parse_args()
    print(json.dumps(rewrite(args.source, args.destination, args.cover), ensure_ascii=False, indent=2))
