import importlib.util
import io
from pathlib import Path
import tempfile
import unittest
import xml.etree.ElementTree as ET
import zipfile

from PIL import Image

spec = importlib.util.spec_from_file_location('embedding', Path(__file__).with_name('embed-epub-cover.py'))
embedding = importlib.util.module_from_spec(spec); spec.loader.exec_module(embedding)


class EmbeddedCoverTests(unittest.TestCase):
    def fixture(self, directory, media='image/jpeg', version='2.0'):
        root = Path(directory); source = root / 'original.epub'; cover = root / 'replacement.jpg'
        Image.new('RGB', (60,90), '#345678').save(cover)
        image = '<item id="art" href="cover.' + ('png' if media == 'image/png' else 'jpg') + '" media-type="' + media + '"/>' if media else ''
        meta = '<meta name="cover" content="art"/>' if media else ''
        package = f'<package xmlns="{embedding.OPF}" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" version="{version}"><metadata><dc:title>Original title</dc:title><dc:creator>Author</dc:creator><meta property="dcterms:modified">2020-01-01T00:00:00Z</meta>{meta}</metadata><manifest>{image}<item id="story" href="story.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="story"/></spine></package>'
        with zipfile.ZipFile(source,'w') as z:
            z.writestr('mimetype','application/epub+zip',compress_type=zipfile.ZIP_STORED)
            z.writestr('META-INF/container.xml',f'<container xmlns="{embedding.CONTAINER}"><rootfiles><rootfile full-path="OPS/package.opf"/></rootfiles></container>')
            z.writestr('OPS/package.opf',package)
            z.writestr('OPS/story.xhtml','<html xmlns="http://www.w3.org/1999/xhtml"><body><p>Story text: 名 ß.</p></body></html>')
            z.writestr('license.txt',b'Original provenance and license')
            if media:
                data=io.BytesIO();Image.new('RGB',(10,15),'red').save(data,'PNG' if media=='image/png' else 'JPEG');z.writestr('OPS/cover.png' if media=='image/png' else 'OPS/cover.jpg',data.getvalue())
        return source, cover

    def test_existing_jpeg_slot_changes_only_the_cover(self):
        with tempfile.TemporaryDirectory() as temporary:
            source,cover=self.fixture(temporary); original=source.read_bytes();output=Path(temporary)/'updated.epub'
            report=embedding.rewrite(source,output,cover)
            self.assertEqual(report['changed_entries'],['OPS/cover.jpg']);self.assertEqual(source.read_bytes(),original)
            with zipfile.ZipFile(source) as before,zipfile.ZipFile(output) as after:
                self.assertEqual(after.read('OPS/cover.jpg'),cover.read_bytes())
                for name in before.namelist():
                    if name!='OPS/cover.jpg':self.assertEqual(after.read(name),before.read(name))

    def test_png_slot_preserves_format_and_new_image_pixels(self):
        with tempfile.TemporaryDirectory() as temporary:
            source,cover=self.fixture(temporary,'image/png');output=Path(temporary)/'updated.epub';embedding.rewrite(source,output,cover)
            with zipfile.ZipFile(output) as archive,Image.open(cover) as expected,Image.open(io.BytesIO(archive.read('OPS/cover.png'))) as actual:
                self.assertEqual(actual.format,'PNG');self.assertEqual(actual.size,expected.size);self.assertEqual(actual.convert('RGB').tobytes(),expected.convert('RGB').tobytes())

    def test_coverless_epub3_gets_cover_metadata_and_first_page(self):
        with tempfile.TemporaryDirectory() as temporary:
            source,cover=self.fixture(temporary,None,'3.0');output=Path(temporary)/'updated.epub';report=embedding.rewrite(source,output,cover)
            with zipfile.ZipFile(output) as archive:
                package_bytes=archive.read('OPS/package.opf');package=ET.fromstring(package_bytes);item=embedding.cover_item(package)
                self.assertEqual(item.get('properties'),'cover-image');self.assertIn(b'xmlns:dcterms=',package_bytes)
                self.assertEqual(package.find('{'+embedding.OPF+'}metadata/{http://purl.org/dc/elements/1.1/}title').text,'Original title')
                first=package.find('{'+embedding.OPF+'}spine')[0].get('idref');page=next(i for i in package.find('{'+embedding.OPF+'}manifest') if i.get('id')==first)
                self.assertEqual(page.get('media-type'),'application/xhtml+xml');self.assertIn(report['embedded_cover_path'],archive.namelist())
                self.assertIn('Story text: 名 ß.',archive.read('OPS/story.xhtml').decode())

    def test_original_cannot_be_overwritten(self):
        with tempfile.TemporaryDirectory() as temporary:
            source,cover=self.fixture(temporary);original=source.read_bytes()
            with self.assertRaises(ValueError):embedding.rewrite(source,source,cover)
            self.assertEqual(source.read_bytes(),original)

    def test_short_prose_mislabelled_as_cover_is_preserved_in_reading_order(self):
        with tempfile.TemporaryDirectory() as temporary:
            source, cover = self.fixture(temporary, None, '3.0')
            with zipfile.ZipFile(source) as archive:
                contents = {name: archive.read(name) for name in archive.namelist()}
            package = ET.fromstring(contents['OPS/package.opf'])
            metadata = package.find(f'{{{embedding.OPF}}}metadata')
            ET.SubElement(metadata, f'{{{embedding.OPF}}}meta', {'name': 'cover', 'content': 'story'})
            contents['OPS/package.opf'] = ET.tostring(package)
            with zipfile.ZipFile(source, 'w') as archive:
                for name, content in contents.items(): archive.writestr(name, content)
            output = Path(temporary) / 'updated.epub'
            report = embedding.rewrite(source, output, cover)
            self.assertNotIn('OPS/story.xhtml', report['changed_entries'])
            with zipfile.ZipFile(output) as archive:
                self.assertEqual(archive.read('OPS/story.xhtml'), contents['OPS/story.xhtml'])
                updated = ET.fromstring(archive.read('OPS/package.opf'))
                self.assertEqual(updated.find(f'{{{embedding.OPF}}}spine')[1].get('idref'), 'story')

    def test_image_only_cover_leaf_is_updated_without_losing_story(self):
        with tempfile.TemporaryDirectory() as temporary:
            source, cover = self.fixture(temporary, None, '3.0')
            with zipfile.ZipFile(source) as archive:
                contents = {name: archive.read(name) for name in archive.namelist()}
            package = ET.fromstring(contents['OPS/package.opf'])
            ET.SubElement(package.find(f'{{{embedding.OPF}}}metadata'), f'{{{embedding.OPF}}}meta', {'name': 'cover', 'content': 'cover-leaf'})
            ET.SubElement(package.find(f'{{{embedding.OPF}}}manifest'), f'{{{embedding.OPF}}}item', {'id': 'cover-leaf', 'href': 'cover.xhtml', 'media-type': 'application/xhtml+xml'})
            package.find(f'{{{embedding.OPF}}}spine').insert(0, ET.Element(f'{{{embedding.OPF}}}itemref', {'idref': 'cover-leaf'}))
            contents['OPS/package.opf'] = ET.tostring(package)
            contents['OPS/cover.xhtml'] = f'<html xmlns="{embedding.XHTML}"><head><title>Cover</title></head><body><img src="old.jpg" alt="Cover"/></body></html>'.encode()
            with zipfile.ZipFile(source, 'w') as archive:
                for name, content in contents.items(): archive.writestr(name, content)
            output = Path(temporary) / 'updated.epub'
            report = embedding.rewrite(source, output, cover)
            self.assertIn('OPS/cover.xhtml', report['changed_entries'])
            with zipfile.ZipFile(output) as archive:
                self.assertEqual(archive.read('OPS/story.xhtml'), contents['OPS/story.xhtml'])
                updated = ET.fromstring(archive.read('OPS/package.opf'))
                refs = [ref.get('idref') for ref in updated.find(f'{{{embedding.OPF}}}spine')]
                self.assertEqual(refs[1:], ['story'])
                self.assertNotIn(b'old.jpg', archive.read('OPS/cover.xhtml'))


if __name__=='__main__':unittest.main()
