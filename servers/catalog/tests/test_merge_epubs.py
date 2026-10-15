"""Exercise collisions, navigation, source preservation and failure handling."""
import importlib.util
import hashlib
from pathlib import Path
import sys
import tempfile
import unittest
import xml.etree.ElementTree as ET
import zipfile


SPEC = importlib.util.spec_from_file_location('merge_epubs', Path(__file__).parents[1] / 'tools/merge-epubs.py')
merge = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = merge
SPEC.loader.exec_module(merge)


def fixture(path, number, language='en', version='3.0', encrypted=False, broken=False):
    files = {
        'META-INF/container.xml': b'<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>',
        'OEBPS/content.opf': f'''<package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/" version="{version}" unique-identifier="uid">
          <metadata><dc:identifier id="uid">urn:volume:{number}</dc:identifier><dc:title>A Work, Volume {number}</dc:title><dc:creator id="author">An Author</dc:creator><meta refines="#author" property="role" scheme="marc:relators">aut</meta><dc:language>{language}</dc:language><meta property="dcterms:modified">2026-01-01T00:00:00Z</meta></metadata>
          <manifest><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/><item id="notes" href="notes.xhtml" media-type="application/xhtml+xml"/><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="style" href="css/style.css" media-type="text/css"/><item id="cover" href="images/cover.png" media-type="image/png" properties="cover-image"/></manifest>
          <spine><itemref id="reading" idref="chapter"/><itemref idref="notes"/></spine></package>'''.encode(),
        'OEBPS/chapter.xhtml': f'''<html xmlns="http://www.w3.org/1999/xhtml"><head><title>Chapter {number}</title><link rel="stylesheet" href="css/style.css"/></head><body><h1 id="chapter">Chapter {number}</h1><p id="reference">Volume-specific text {number}: <a href="notes.xhtml#{'missing' if broken else 'note'}">a footnote</a></p><img src="images/cover.png" alt="Cover"/></body></html>'''.encode(),
        'OEBPS/notes.xhtml': f'<html xmlns="http://www.w3.org/1999/xhtml"><head><title>Notes</title></head><body><p id="note">Note {number} <a href="chapter.xhtml#reference">back</a></p></body></html>'.encode(),
        'OEBPS/nav.xhtml': f'''<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><head><title>Contents</title></head><body><nav epub:type="toc"><ol><li><a id="same-nav-id" href="chapter.xhtml#chapter">Chapter {number}</a><ol><li><a href="notes.xhtml#note">Notes {number}</a></li></ol></li></ol></nav><nav epub:type="page-list"><ol><li><a href="chapter.xhtml#chapter">1</a></li></ol></nav></body></html>'''.encode(),
        'OEBPS/css/style.css': b'body { background-image: url("../images/cover.png"); }',
        'OEBPS/images/cover.png': b'volume-specific image ' + str(number).encode(),
    }
    if encrypted:
        files['META-INF/encryption.xml'] = b'<encryption/>'
    with zipfile.ZipFile(path, 'w', compression=zipfile.ZIP_DEFLATED) as archive:
        archive.writestr('mimetype', b'application/epub+zip', compress_type=zipfile.ZIP_STORED)
        for name, data in files.items():
            archive.writestr(name, data)
    return files


class MergeTests(unittest.TestCase):
    def test_ncx_repeated_targets_share_play_order_without_losing_labels(self):
        entries = [{'label': 'First original label', 'href': 'chapter.xhtml#start', 'children': [
            {'label': 'Second original label', 'href': 'chapter.xhtml#start', 'children': []}]},
            {'label': 'Following chapter', 'href': 'next.xhtml', 'children': []}]
        ncx = ET.fromstring(merge.make_ncx('A Work', 'urn:test', entries))
        points = list(ncx.iter(merge.tag(merge.NCX, 'navPoint')))
        self.assertEqual([p.get('playOrder') for p in points], ['1', '1', '2'])
        self.assertEqual(len({p.get('id') for p in points}), 3)
        self.assertEqual([p.findtext('.//' + merge.tag(merge.NCX, 'text')) for p in points],
                         ['First original label', 'Second original label', 'Following chapter'])

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.paths = [self.root / 'one.epub', self.root / 'two.epub']
        self.inputs = [fixture(path, i) for i, path in enumerate(self.paths, 1)]
        self.before = [path.read_bytes() for path in self.paths]

    def run_merge(self, output=None):
        return merge.merge_epubs(self.paths, output or self.root / 'combined.epub', 'A Work', modified='2026-10-12T00:00:00Z')

    def rewrite(self, path, replacements):
        with zipfile.ZipFile(path) as archive:
            files = {item.filename: archive.read(item.filename) for item in archive.infolist()}
        for name, data in replacements.items():
            files[name] = data
        with zipfile.ZipFile(path, 'w') as archive:
            for name, data in files.items():
                archive.writestr(name, data)

    def review(self):
        volumes = [merge.read_volume(p, str(i), i) for i, p in enumerate(self.paths, 1)]
        return {'work': 'A Work', 'edition_evidence': 'Reviewed matching titlepages and complete two-volume edition.',
                'sources': [{'sha256': v.sha256, 'creators': list(v.creators), 'contributors': list(v.contributors),
                             'languages': list(v.languages)} for v in volumes]}

    def test_reviewed_navigation_target_repair_keeps_original_label_and_source_bytes(self):
        navigation = self.inputs[0]['OEBPS/nav.xhtml'].replace(b'chapter.xhtml#chapter', b'chapter.xhtml#reference')
        self.rewrite(self.paths[0], {'OEBPS/nav.xhtml': navigation})
        review = self.review()
        volume = merge.read_volume(self.paths[0], '1', 1)
        review['navigation_target_repairs'] = [{
            'source_sha256': volume.sha256, 'href': volume.toc[0]['href'],
            'label': 'Chapter 1', 'target_href': volume.prefix + 'OEBPS/chapter.xhtml#chapter',
            'original_heading': 'Chapter 1', 'reason': 'Verify original heading target.'}]
        report = merge.merge_epubs(self.paths, self.root / 'repaired-nav.epub', 'A Work', review=review)
        with zipfile.ZipFile(report['output']) as archive:
            self.assertEqual(archive.read('volumes/v001/OEBPS/chapter.xhtml'), self.inputs[0]['OEBPS/chapter.xhtml'])
            nav = ET.fromstring(archive.read('nav.xhtml'))
            chapter = next(e for e in nav.iter(merge.tag(merge.XHTML, 'a')) if e.text == 'Chapter 1')
            self.assertEqual(chapter.get('href'), volume.prefix + 'OEBPS/chapter.xhtml#chapter')
        review['navigation_target_repairs'][0]['target_href'] = 'volumes/v001/OEBPS/notes.xhtml#note'
        with self.assertRaisesRegex(ValueError, 'exact original heading'):
            merge.merge_epubs(self.paths, self.root / 'wrong-target.epub', 'A Work', review=review)
        review['navigation_target_repairs'][0]['target_href'] = 'https://example.org/chapter'
        with self.assertRaisesRegex(ValueError, 'inside its original volume'):
            merge.merge_epubs(self.paths, self.root / 'external-target.epub', 'A Work', review=review)

    def omitted_parts_review(self):
        document = self.inputs[0]['OEBPS/chapter.xhtml'].replace(b'</body>',
            b'<h5><a id="part-one">PART I.</a></h5><p>First part text.</p>'
            b'<h5><a id="part-two">PART II.</a></h5><p>Second part text.</p></body>')
        self.rewrite(self.paths[0], {'OEBPS/chapter.xhtml': document})
        review = self.review()
        volume = merge.read_volume(self.paths[0], '1', 1)
        review['added_navigation'] = [
            {'source_sha256': volume.sha256, 'member': 'OEBPS/chapter.xhtml',
             'target_id': target, 'label': label, 'parent_href': volume.toc[0]['href'],
             'parent_label': volume.toc[0]['label'], 'reason': 'Recover an omitted original part heading.'}
            for target, label in [('part-two', 'PART II.'), ('part-one', 'PART I.')]]
        return review, document

    def test_added_navigation_restores_original_parts_in_reading_order_without_changing_text(self):
        review, document = self.omitted_parts_review()
        report = merge.merge_epubs(self.paths, self.root / 'parts.epub', 'A Work', review=review)
        with zipfile.ZipFile(report['output']) as archive:
            self.assertEqual(archive.read('volumes/v001/OEBPS/chapter.xhtml'), document)
            navigation = ET.fromstring(archive.read('nav.xhtml'))
            labels = [a.text for a in navigation.iter(merge.tag(merge.XHTML, 'a'))]
        self.assertEqual(labels[:4], ['Chapter 1', 'PART I.', 'PART II.', 'Notes 1'])

    def test_added_navigation_rejects_invented_heading_and_missing_anchor(self):
        review, _ = self.omitted_parts_review()
        review['added_navigation'][0]['label'] = 'A fabricated title'
        with self.assertRaisesRegex(ValueError, 'exact original heading'):
            merge.merge_epubs(self.paths, self.root / 'bad.epub', 'A Work', review=review)
        review['added_navigation'][0]['label'] = 'PART II.'
        review['added_navigation'][0]['target_id'] = 'missing'
        with self.assertRaisesRegex(ValueError, 'unique source anchor'):
            merge.merge_epubs(self.paths, self.root / 'bad.epub', 'A Work', review=review)

    def test_added_navigation_requires_original_parent_and_unique_review(self):
        review, _ = self.omitted_parts_review()
        review['added_navigation'][0]['parent_label'] = 'Unreviewed parent'
        with self.assertRaisesRegex(ValueError, 'original parent'):
            merge.merge_epubs(self.paths, self.root / 'bad.epub', 'A Work', review=review)
        review['added_navigation'][0]['parent_label'] = 'Chapter 1'
        review['added_navigation'] *= 2
        with self.assertRaisesRegex(ValueError, 'unique reviewed heading'):
            merge.merge_epubs(self.paths, self.root / 'bad.epub', 'A Work', review=review)

    def test_added_navigation_reuses_adjacent_original_title_without_inventing_one(self):
        review, document = self.omitted_parts_review()
        document = document.replace(b'<p>First part text.</p>', b'<h5>Original section title.</h5><p>First part text.</p>')
        self.rewrite(self.paths[0], {'OEBPS/chapter.xhtml': document})
        refreshed = self.review()
        entry = dict(review['added_navigation'][1], source_sha256=refreshed['sources'][0]['sha256'],
                     adjacent_heading=True, label='Original section title.')
        refreshed['added_navigation'] = [entry]
        result = merge.merge_epubs(self.paths, self.root / 'titled.epub', 'A Work', review=refreshed)
        with zipfile.ZipFile(result['output']) as archive:
            labels = [a.text for a in ET.fromstring(archive.read('nav.xhtml')).iter(merge.tag(merge.XHTML, 'a'))]
        self.assertIn('Original section title.', labels)
        entry['label'] = 'Invented section title.'
        with self.assertRaisesRegex(ValueError, 'exact original heading'):
            merge.merge_epubs(self.paths, self.root / 'bad.epub', 'A Work', review=refreshed)

    def test_plain_heading_navigation_requires_review_and_preserves_source_text(self):
        document = self.inputs[0]['OEBPS/chapter.xhtml'].replace(b'</body>',
            b'<p class="head3" id="king-one">WILLIAM THE FIRST.</p>'
            b'<div class="fsize2" id="king-two">RICHARD THE SECOND,</div></body>')
        self.rewrite(self.paths[0], {'OEBPS/chapter.xhtml': document})
        review = self.review()
        volume = merge.read_volume(self.paths[0], '1', 1)
        review['added_navigation'] = [
            {'source_sha256': volume.sha256, 'member': 'OEBPS/chapter.xhtml',
             'target_id': target, 'label': label, 'plain_heading': True,
             'parent_href': volume.toc[0]['href'], 'parent_label': volume.toc[0]['label'],
             'reason': 'Printed reign heading is marked as a styled paragraph or plain div.'}
            for target, label in [('king-one', 'WILLIAM THE FIRST.'), ('king-two', 'RICHARD THE SECOND,')]]
        result = merge.merge_epubs(self.paths, self.root / 'plain.epub', 'A Work', review=review)
        with zipfile.ZipFile(result['output']) as archive:
            self.assertEqual(archive.read('volumes/v001/OEBPS/chapter.xhtml'), document)
            labels = [a.text for a in ET.fromstring(archive.read('nav.xhtml')).iter(merge.tag(merge.XHTML, 'a'))]
        self.assertIn('WILLIAM THE FIRST.', labels)
        self.assertIn('RICHARD THE SECOND,', labels)
        review['added_navigation'][0].pop('plain_heading')
        with self.assertRaisesRegex(ValueError, 'exact original heading'):
            merge.merge_epubs(self.paths, self.root / 'unreviewed.epub', 'A Work', review=review)

    def test_plain_heading_navigation_rejects_container_and_invented_label(self):
        document = self.inputs[0]['OEBPS/chapter.xhtml'].replace(b'</body>',
            b'<div id="container"><p>Body paragraph.</p></div></body>')
        self.rewrite(self.paths[0], {'OEBPS/chapter.xhtml': document})
        review = self.review()
        volume = merge.read_volume(self.paths[0], '1', 1)
        entry = {'source_sha256': volume.sha256, 'member': 'OEBPS/chapter.xhtml',
                 'target_id': 'container', 'label': 'Body paragraph.', 'plain_heading': True,
                 'parent_href': volume.toc[0]['href'], 'parent_label': volume.toc[0]['label'],
                 'reason': 'A container is not a printed heading.'}
        review['added_navigation'] = [entry]
        with self.assertRaisesRegex(ValueError, 'exact original heading'):
            merge.merge_epubs(self.paths, self.root / 'container.epub', 'A Work', review=review)
        entry.update(target_id='reference', label='An invented title')
        with self.assertRaisesRegex(ValueError, 'exact original heading'):
            merge.merge_epubs(self.paths, self.root / 'invented.epub', 'A Work', review=review)

    def test_inline_printed_heading_requires_review_and_exact_title(self):
        document = self.inputs[0]['OEBPS/chapter.xhtml'].replace(b'</body>',
            b'<p><span class="printed-title" id="tale">BAUFRA\'S TALE</span> The original story begins here.</p></body>')
        self.rewrite(self.paths[0], {'OEBPS/chapter.xhtml': document})
        review = self.review()
        volume = merge.read_volume(self.paths[0], '1', 1)
        entry = {'source_sha256': volume.sha256, 'member': 'OEBPS/chapter.xhtml',
                 'target_id': 'tale', 'label': "BAUFRA'S TALE", 'inline_heading': True,
                 'parent_href': volume.toc[0]['href'], 'parent_label': volume.toc[0]['label'],
                 'reason': 'Original story title is an inline styled span, also listed in the printed contents.'}
        review['added_navigation'] = [entry]
        result = merge.merge_epubs(self.paths, self.root / 'inline.epub', 'A Work', review=review)
        with zipfile.ZipFile(result['output']) as archive:
            self.assertEqual(archive.read('volumes/v001/OEBPS/chapter.xhtml'), document)
            labels = [a.text for a in ET.fromstring(archive.read('nav.xhtml')).iter(merge.tag(merge.XHTML, 'a'))]
        self.assertIn("BAUFRA'S TALE", labels)
        entry.pop('inline_heading')
        with self.assertRaisesRegex(ValueError, 'exact original heading'):
            merge.merge_epubs(self.paths, self.root / 'unreviewed-inline.epub', 'A Work', review=review)
        entry.update(inline_heading=True, label='An invented story title')
        with self.assertRaisesRegex(ValueError, 'exact original heading'):
            merge.merge_epubs(self.paths, self.root / 'invented-inline.epub', 'A Work', review=review)

    def test_original_italic_story_heading_is_retained_without_reformatting(self):
        document = self.inputs[0]['OEBPS/chapter.xhtml'].replace(b'</body>',
            b'<p><i id="sailor">THE SHIPWRECKED SAILOR</i> The original story.</p></body>')
        self.rewrite(self.paths[0], {'OEBPS/chapter.xhtml': document})
        review = self.review()
        volume = merge.read_volume(self.paths[0], '1', 1)
        review['added_navigation'] = [{'source_sha256': volume.sha256, 'member': 'OEBPS/chapter.xhtml',
            'target_id': 'sailor', 'label': 'THE SHIPWRECKED SAILOR', 'inline_heading': True,
            'parent_href': volume.toc[0]['href'], 'parent_label': volume.toc[0]['label'],
            'reason': 'Original italic title is also listed in the printed contents.'}]
        result = merge.merge_epubs(self.paths, self.root / 'italic.epub', 'A Work', review=review)
        with zipfile.ZipFile(result['output']) as archive:
            self.assertEqual(archive.read('volumes/v001/OEBPS/chapter.xhtml'), document)
            labels = [a.text for a in ET.fromstring(archive.read('nav.xhtml')).iter(merge.tag(merge.XHTML, 'a'))]
        self.assertIn('THE SHIPWRECKED SAILOR', labels)

    def test_source_descriptions_are_preserved_in_the_generated_package(self):
        description = 'Original overview & context.'
        for i, path in enumerate(self.paths):
            metadata = self.inputs[i]['OEBPS/content.opf'].replace(b'</metadata>',
                b'<dc:description>Original overview &amp; context.</dc:description></metadata>')
            self.rewrite(path, {'OEBPS/content.opf': metadata})
        report = merge.merge_epubs(self.paths, self.root / 'described.epub', 'A Work')
        with zipfile.ZipFile(report['output']) as archive:
            md = ET.fromstring(archive.read('content.opf')).find(merge.tag(merge.OPF, 'metadata'))
        self.assertEqual(md.findtext(merge.tag(merge.DC, 'description')), description)
        self.assertEqual(report['description'], description)
        self.assertEqual(report['modified'], '2026-01-01T00:00:00Z')

    def test_distinct_descriptions_keep_their_source_labels_and_change_revision_identity(self):
        retained = merge.combine_descriptions([('Part I', 'First source text.'), ('Part II', 'Second source text.'),
                                               ('Part III', 'First source text.'), ('Part IV', '')])
        self.assertEqual(retained, 'Part I; Part III\nFirst source text.\n\nPart II\nSecond source text.')
        a = merge.merge_epubs(self.paths, self.root / 'a.epub', 'A Work', description=retained)
        b = merge.merge_epubs(self.paths, self.root / 'b.epub', 'A Work', description='Other source text.')
        self.assertNotEqual(a['identifier'], b['identifier'])
        self.assertEqual(a['modified'], b['modified'])

    def test_reviewed_contributor_variation_preserves_later_credit_and_role(self):
        metadata = self.inputs[1]['OEBPS/content.opf'].replace(b'</metadata>',
            b'<dc:contributor id="translator">A Translator</dc:contributor><meta refines="#translator" property="role" scheme="marc:relators">trl</meta></metadata>')
        self.rewrite(self.paths[1], {'OEBPS/content.opf': metadata})
        with self.assertRaisesRegex(ValueError, 'same credited authors'):
            self.run_merge()
        before = [p.read_bytes() for p in self.paths]
        review = self.review()
        report = merge.merge_epubs(self.paths, self.root / 'reviewed.epub', 'A Work', review=review)
        with zipfile.ZipFile(report['output']) as archive:
            md = ET.fromstring(archive.read('content.opf')).find(merge.tag(merge.OPF, 'metadata'))
        self.assertEqual([e.text for e in md.findall(merge.tag(merge.DC, 'creator'))], ['An Author'])
        credit = md.find(merge.tag(merge.DC, 'contributor'))
        self.assertEqual(credit.text, 'A Translator')
        self.assertTrue(any(e.get('refines') == '#' + credit.get('id') and e.text == 'trl' for e in md))
        self.assertEqual(report['credit_policy'], 'reviewed_contributor_variation')
        self.assertEqual([p.read_bytes() for p in self.paths], before)
        review['sources'][1]['sha256'] = 'wrong-file'
        with self.assertRaisesRegex(ValueError, 'review does not match'):
            merge.merge_epubs(self.paths, self.root / 'stale-review.epub', 'A Work', review=review)

    def test_ordinary_review_does_not_allow_different_authors_or_languages(self):
        self.rewrite(self.paths[1], {'OEBPS/content.opf': self.inputs[1]['OEBPS/content.opf'].replace(b'An Author', b'Another Author')})
        with self.assertRaisesRegex(ValueError, 'same credited authors'):
            merge.merge_epubs(self.paths, self.root / 'wrong-author.epub', 'A Work', review=self.review())
        fixture(self.paths[1], 2, language='sv')
        with self.assertRaisesRegex(ValueError, 'same credited authors'):
            merge.merge_epubs(self.paths, self.root / 'wrong-language.epub', 'A Work', review=self.review())

    def test_explicit_language_review_preserves_all_source_languages(self):
        fixture(self.paths[1], 2, language='enm')
        review = self.review()
        review['language_variation'] = {'languages': ['en', 'enm'],
            'evidence': 'One matching edition includes Middle English text and English commentary.'}
        before = [p.read_bytes() for p in self.paths]
        report = merge.merge_epubs(self.paths, self.root / 'mixed.epub', 'A Work', review=review)
        with zipfile.ZipFile(report['output']) as archive:
            md = ET.fromstring(archive.read('content.opf')).find(merge.tag(merge.OPF, 'metadata'))
        self.assertEqual([e.text for e in md.findall(merge.tag(merge.DC, 'language'))], ['en', 'enm'])
        self.assertEqual([p.read_bytes() for p in self.paths], before)
        review['language_variation']['languages'] = ['en']
        with self.assertRaisesRegex(ValueError, 'exact source language union'):
            merge.merge_epubs(self.paths, self.root / 'lost-language.epub', 'A Work', review=review)
        review['language_variation'] = {'languages': ['en', 'enm'], 'evidence': ''}
        with self.assertRaisesRegex(ValueError, 'Language review needs edition evidence'):
            merge.merge_epubs(self.paths, self.root / 'missing-evidence.epub', 'A Work', review=review)
        self.rewrite(self.paths[1], {'OEBPS/content.opf': self.inputs[1]['OEBPS/content.opf']
            .replace(b'An Author', b'Another Author').replace(b'>en<', b'>enm<')})
        review = self.review()
        review['language_variation'] = {'languages': ['en', 'enm'], 'evidence': 'Reviewed languages.'}
        with self.assertRaisesRegex(ValueError, 'same credited authors'):
            merge.merge_epubs(self.paths, self.root / 'wrong-author-mixed.epub', 'A Work', review=review)

    def test_reviewed_markup_repair_is_confined_to_generated_copy_and_keeps_text(self):
        name = 'OEBPS/chapter.xhtml'
        data = self.inputs[1][name].replace(b'</body>', b'<table><tr><th colspan="0">Titles</th></tr><tr><td>A</td><td>B</td></tr></table></body>')
        self.rewrite(self.paths[1], {name: data})
        before = [p.read_bytes() for p in self.paths]
        review = self.review()
        review['repairs'] = [{'source_sha256': review['sources'][1]['sha256'], 'member': name,
            'member_sha256': hashlib.sha256(data).hexdigest(), 'reason': 'Table has two columns; zero colspan is invalid in EPUB3.',
            'replacements': [{'old': 'colspan="0"', 'new': 'colspan="2"', 'count': 1}]}]
        report = merge.merge_epubs(self.paths, self.root / 'repaired.epub', 'A Work', review=review)
        with zipfile.ZipFile(report['output']) as archive:
            repaired = archive.read('volumes/v002/' + name)
        self.assertIn(b'colspan="2"', repaired)
        self.assertEqual(''.join(ET.fromstring(data).itertext()), ''.join(ET.fromstring(repaired).itertext()))
        self.assertEqual([p.read_bytes() for p in self.paths], before)
        self.assertEqual(report['repaired_resources'], ['volumes/v002/' + name])
        self.assertEqual(report['anchor_only_modified_resources'], [])
        self.assertFalse(report['content_preserved'])
        review['repairs'][0]['replacements'][0] = {'old': '>Titles<', 'new': '>Invented title<', 'count': 1}
        with self.assertRaisesRegex(ValueError, 'changed source text'):
            merge.merge_epubs(self.paths, self.root / 'changed-text.epub', 'A Work', review=review)

    def test_reviewed_author_role_variation_preserves_both_roles(self):
        metadata = self.inputs[1]['OEBPS/content.opf'].replace(b'dc:creator', b'dc:contributor').replace(b'>aut<', b'>edt<')
        self.rewrite(self.paths[1], {'OEBPS/content.opf': metadata})
        review = self.review()
        with self.assertRaisesRegex(ValueError, 'same credited authors'):
            merge.merge_epubs(self.paths, self.root / 'unreviewed-roles.epub', 'A Work', review=review)
        review['creator_variation'] = {'creators': ['An Author'], 'evidence': 'Same series author edits the final volume.'}
        report = merge.merge_epubs(self.paths, self.root / 'roles.epub', 'A Work', review=review)
        with zipfile.ZipFile(report['output']) as archive:
            md = ET.fromstring(archive.read('content.opf')).find(merge.tag(merge.OPF, 'metadata'))
        self.assertEqual(md.findtext(merge.tag(merge.DC, 'creator')), 'An Author')
        self.assertEqual(md.findtext(merge.tag(merge.DC, 'contributor')), 'An Author')
        self.assertEqual(sorted(e.text for e in md if e.get('property') == 'role'), ['aut', 'edt'])
        review['creator_variation']['creators'] = []
        with self.assertRaisesRegex(ValueError, 'exact source creator union'):
            merge.merge_epubs(self.paths, self.root / 'wrong-creators.epub', 'A Work', review=review)

    def test_reviewed_credit_requires_original_text_evidence(self):
        member = 'OEBPS/chapter.xhtml'
        data = self.inputs[1][member].replace(b'</body>', b'<p>Essay by A Contributor.</p></body>')
        self.rewrite(self.paths[1], {member: data})
        review = self.review()
        review['additional_credits'] = [{'source_sha256': review['sources'][1]['sha256'], 'member': member,
            'name': 'A Contributor', 'role': 'ctb', 'evidence': 'Essay by A Contributor.'}]
        report = merge.merge_epubs(self.paths, self.root / 'credited.epub', 'A Work', review=review)
        with zipfile.ZipFile(report['output']) as archive:
            md = ET.fromstring(archive.read('content.opf')).find(merge.tag(merge.OPF, 'metadata'))
            self.assertEqual(md.findtext(merge.tag(merge.DC, 'contributor')), 'A Contributor')
            self.assertEqual(archive.read('volumes/v002/' + member), data)
        review['additional_credits'][0]['evidence'] = 'A nonexistent byline.'
        with self.assertRaisesRegex(ValueError, 'evidence is absent'):
            merge.merge_epubs(self.paths, self.root / 'invented-credit.epub', 'A Work', review=review)

    def test_original_illustrator_byline_is_retained_with_illustrator_role(self):
        member = 'OEBPS/chapter.xhtml'
        data = self.inputs[0][member].replace(b'</body>', b'<p>ILLUSTRATED BY TRISTRAM ELLIS</p></body>')
        self.rewrite(self.paths[0], {member: data})
        review = self.review()
        review['additional_credits'] = [{'source_sha256': review['sources'][0]['sha256'], 'member': member,
            'name': 'Tristram Ellis', 'role': 'ill', 'evidence': 'ILLUSTRATED BY TRISTRAM ELLIS'}]
        report = merge.merge_epubs(self.paths, self.root / 'illustrated.epub', 'A Work', review=review)
        with zipfile.ZipFile(report['output']) as archive:
            md = ET.fromstring(archive.read('content.opf')).find(merge.tag(merge.OPF, 'metadata'))
            credit = next(n for n in md if n.text == 'Tristram Ellis')
            role = next(n for n in md if n.get('refines') == '#' + credit.get('id') and n.get('property') == 'role')
            self.assertEqual(role.text, 'ill')
            self.assertEqual(archive.read('volumes/v001/' + member), data)

    def test_missing_source_image_can_be_restored_from_reviewed_bytes(self):
        import base64
        asset = self.root / 'map.png'
        data = base64.b64decode('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jD1EAAAAASUVORK5CYII=')
        asset.write_bytes(data)
        member = 'OEBPS/chapter.xhtml'
        self.rewrite(self.paths[0], {member: self.inputs[0][member].replace(b'images/cover.png', b'images/missing.png')})
        review = self.review()
        review['additional_assets'] = [{'source_sha256': review['sources'][0]['sha256'],
            'member': 'OEBPS/images/missing.png', 'path': str(asset), 'sha256': hashlib.sha256(data).hexdigest(),
            'media_type': 'image/png', 'evidence': 'Actual source map omitted by the EPUB generator.'}]
        report = merge.merge_epubs(self.paths, self.root / 'restored-map.epub', 'A Work', review=review)
        with zipfile.ZipFile(report['output']) as archive:
            self.assertEqual(archive.read('volumes/v001/OEBPS/images/missing.png'), data)
            self.assertEqual(archive.read('volumes/v001/' + member),
                             self.inputs[0][member].replace(b'images/cover.png', b'images/missing.png'))
        asset.write_bytes(b'wrong bytes')
        with self.assertRaisesRegex(ValueError, 'retained hash or format'):
            merge.merge_epubs(self.paths, self.root / 'wrong-map.epub', 'A Work', review=review)

    def test_reviewed_navigation_flattening_promotes_original_children_only(self):
        review = self.review()
        volume = merge.read_volume(self.paths[0], 'Volume I', 1)
        entry = volume.toc[0]
        review['flattened_navigation'] = [{'source_sha256': volume.sha256,
            'label': entry['label'], 'href': entry['href'], 'reason': 'Reviewed volume wrapper.'}]
        report = merge.merge_epubs(self.paths, self.root / 'flat.epub', 'A Work', review=review)
        with zipfile.ZipFile(report['output']) as archive:
            nav = ET.fromstring(archive.read('nav.xhtml'))
            toc = next(e for e in nav.iter(merge.tag(merge.XHTML, 'nav'))
                       if e.get(merge.tag(merge.EPUB, 'type')) == 'toc')
            labels = [e.find(merge.tag(merge.XHTML, 'a')).text
                      for e in toc.find(merge.tag(merge.XHTML, 'ol'))]
            self.assertEqual(labels, ['Notes 1', 'Chapter 2'])
            self.assertEqual(archive.read('volumes/v001/OEBPS/chapter.xhtml'), self.inputs[0]['OEBPS/chapter.xhtml'])
        review['flattened_navigation'][0]['href'] = 'wrong-target.xhtml'
        with self.assertRaisesRegex(ValueError, 'Navigation review must identify'):
            merge.merge_epubs(self.paths, self.root / 'wrong-nav.epub', 'A Work', review=review)

    def test_reviewed_repair_rejects_stale_resource_and_wrong_replacement_count(self):
        review = self.review()
        repair = {'source_sha256': review['sources'][0]['sha256'], 'member': 'OEBPS/chapter.xhtml',
                  'member_sha256': 'stale', 'reason': 'Reviewed repair',
                  'replacements': [{'old': 'id="chapter"', 'new': 'id="chapter"', 'count': 2}]}
        review['repairs'] = [repair]
        with self.assertRaisesRegex(ValueError, 'differs from the reviewed bytes'):
            merge.merge_epubs(self.paths, self.root / 'stale-resource.epub', 'A Work', review=review)
        repair['member_sha256'] = hashlib.sha256(self.inputs[0]['OEBPS/chapter.xhtml']).hexdigest()
        with self.assertRaisesRegex(ValueError, 'replacement count differs'):
            merge.merge_epubs(self.paths, self.root / 'wrong-count.epub', 'A Work', review=review)

    def test_nested_reviewed_wrappers_promote_chapters_without_losing_hierarchy(self):
        leaf = {'label': 'An Original Chapter', 'href': 'chapter.xhtml#chapter', 'children': []}
        inner = {'label': 'Original Volume', 'href': 'title.xhtml#volume', 'children': [leaf]}
        outer = {'label': 'Original Editor', 'href': 'title.xhtml#editor', 'children': [inner]}
        selected = {(inner['href'], inner['label']), (outer['href'], outer['label'])}
        self.assertEqual(merge.flatten_navigation([outer], selected), [leaf])
        self.assertEqual(outer['children'][0]['children'], [leaf])
        self.assertEqual(list(merge.navigation_nodes([outer])), [outer, inner, leaf])

    def test_lifted_frontmatter_keeps_its_target_and_original_chapter_hierarchy(self):
        subtitle = {'label': 'Original Subtitle', 'href': 'chapter.xhtml#subtitle', 'children': []}
        chapter = {'label': 'Chapter I', 'href': 'chapter.xhtml#chapter', 'children': [subtitle]}
        preface = {'label': 'Preface', 'href': 'preface.xhtml#preface', 'children': [chapter]}
        title = {'label': 'Volume I', 'href': 'title.xhtml#volume', 'children': [preface]}
        result = merge.flatten_navigation([title], {(title['href'], title['label'])},
                                          {(preface['href'], preface['label'])})
        self.assertEqual(result, [{**preface, 'children': []}, chapter])
        self.assertEqual(preface['children'], [chapter])

    def test_lifted_navigation_requires_exact_review_and_cannot_also_be_removed(self):
        review = self.review()
        volume = merge.read_volume(self.paths[0], 'Volume I', 1)
        entry = volume.toc[0]
        lifted = {'source_sha256': volume.sha256, 'label': entry['label'], 'href': entry['href'],
                  'reason': 'Reviewed frontmatter heading incorrectly encloses subsequent chapters.'}
        review['lifted_navigation'] = [lifted]
        report = merge.merge_epubs(self.paths, self.root / 'lifted.epub', 'A Work', review=review)
        with zipfile.ZipFile(report['output']) as archive:
            nav = ET.fromstring(archive.read('nav.xhtml'))
            toc = next(e for e in nav.iter(merge.tag(merge.XHTML, 'nav'))
                       if e.get(merge.tag(merge.EPUB, 'type')) == 'toc')
            self.assertEqual([e.find(merge.tag(merge.XHTML, 'a')).text
                              for e in toc.find(merge.tag(merge.XHTML, 'ol'))],
                             ['Chapter 1', 'Notes 1', 'Chapter 2'])
            self.assertEqual(archive.read('volumes/v001/OEBPS/chapter.xhtml'), self.inputs[0]['OEBPS/chapter.xhtml'])
        review['flattened_navigation'] = [lifted]
        with self.assertRaisesRegex(ValueError, 'Navigation review must identify'):
            merge.merge_epubs(self.paths, self.root / 'conflicting-nav.epub', 'A Work', review=review)
        review.pop('flattened_navigation')
        lifted['href'] = 'wrong-target.xhtml'
        with self.assertRaisesRegex(ValueError, 'Navigation review must identify'):
            merge.merge_epubs(self.paths, self.root / 'unknown-lifted-nav.epub', 'A Work', review=review)

    def test_empty_document_title_can_only_reuse_exact_source_title_without_body_changes(self):
        name = 'OEBPS/chapter.xhtml'
        data = self.inputs[0][name].replace(b'<title>Chapter 1</title>', b'<title/>')
        self.rewrite(self.paths[0], {name: data})
        review = self.review()
        repair = {'source_sha256': review['sources'][0]['sha256'], 'member': name,
                  'member_sha256': hashlib.sha256(data).hexdigest(), 'reason': 'Restore empty document metadata from its original package title.',
                  'restore_empty_document_title': True,
                  'replacements': [{'old': '<title/>', 'new': '<title>A Work, Volume 1</title>', 'count': 1}]}
        review['repairs'] = [repair]
        report = merge.merge_epubs(self.paths, self.root / 'head-title.epub', 'A Work', review=review)
        with zipfile.ZipFile(report['output']) as z:
            result = ET.fromstring(z.read('volumes/v001/' + name))
        self.assertEqual(''.join(result.find(merge.tag(merge.XHTML, 'body')).itertext()),
                         ''.join(ET.fromstring(data).find(merge.tag(merge.XHTML, 'body')).itertext()))
        applied = report['volumes'][0]['applied_repairs'][0]
        self.assertFalse(applied['text_preserved'])
        self.assertTrue(applied['reading_text_preserved'])
        self.assertEqual(applied['restored_document_title'], 'A Work, Volume 1')
        repair['replacements'][0]['new'] = '<title>Invented title</title>'
        with self.assertRaisesRegex(ValueError, 'changed source text'):
            merge.merge_epubs(self.paths, self.root / 'invented-title.epub', 'A Work', review=review)
        repair['replacements'][0]['new'] = '<title>A Work, Volume 1</title>'
        repair['replacements'].append({'old': 'Volume-specific text 1', 'new': 'Altered book text', 'count': 1})
        with self.assertRaisesRegex(ValueError, 'changed source text'):
            merge.merge_epubs(self.paths, self.root / 'altered-body.epub', 'A Work', review=review)

    def test_editor_only_volumes_preserve_credits_and_require_matching_editors(self):
        for path in self.paths:
            with zipfile.ZipFile(path) as archive:
                files = {item.filename: archive.read(item.filename) for item in archive.infolist()}
            files['OEBPS/content.opf'] = (files['OEBPS/content.opf']
                .replace(b'dc:creator', b'dc:contributor')
                .replace(b'An Author', b'An Editor').replace(b'>aut<', b'>edt<'))
            with zipfile.ZipFile(path, 'w') as archive:
                for name, data in files.items():
                    archive.writestr(name, data)
        report = self.run_merge()
        with zipfile.ZipFile(report['output']) as archive:
            metadata = ET.fromstring(archive.read('content.opf')).find(merge.tag(merge.OPF, 'metadata'))
        self.assertEqual(metadata.findall(merge.tag(merge.DC, 'creator')), [])
        editor = metadata.find(merge.tag(merge.DC, 'contributor'))
        self.assertEqual(editor.text, 'An Editor')
        role = next(element for element in metadata if element.get('property') == 'role')
        self.assertEqual(role.text, 'edt')
        self.assertEqual(role.get('refines'), '#' + editor.get('id'))
        with zipfile.ZipFile(self.paths[1]) as archive:
            files = {item.filename: archive.read(item.filename) for item in archive.infolist()}
        files['OEBPS/content.opf'] = files['OEBPS/content.opf'].replace(b'An Editor', b'Another Editor')
        with zipfile.ZipFile(self.paths[1], 'w') as archive:
            for name, data in files.items():
                archive.writestr(name, data)
        with self.assertRaisesRegex(ValueError, 'same credited authors'):
            self.run_merge(self.root / 'mismatched-editors.epub')

    def heading_review(self):
        review = self.review()
        review['heading_navigation'] = [{
            'source_sha256': review['sources'][0]['sha256'], 'member': 'OEBPS/chapter.xhtml',
            'target_id': 'chapter', 'label': 'Chapter 1',
            'reason': 'Recover the original source chapter heading from page-only navigation.'}]
        return review

    def test_heading_navigation_preserves_original_text_and_other_volume_toc(self):
        review = self.heading_review()
        report = merge.merge_epubs(self.paths, self.root / 'headings.epub', 'A Work', review=review)
        with zipfile.ZipFile(report['output']) as archive:
            nav = ET.fromstring(archive.read('nav.xhtml'))
            toc = next(e for e in nav.iter(merge.tag(merge.XHTML, 'nav')) if e.get(merge.tag(merge.EPUB, 'type')) == 'toc')
            links = [(a.text, a.get('href')) for a in toc.iter(merge.tag(merge.XHTML, 'a'))]
            self.assertEqual(links, [('Chapter 1', 'volumes/v001/OEBPS/chapter.xhtml#chapter'),
                                    ('Chapter 2', 'volumes/v002/OEBPS/chapter.xhtml#chapter'),
                                    ('Notes 2', 'volumes/v002/OEBPS/notes.xhtml#note')])
            self.assertEqual(archive.read('volumes/v001/OEBPS/chapter.xhtml'), self.inputs[0]['OEBPS/chapter.xhtml'])
        self.assertEqual([p.read_bytes() for p in self.paths], self.before)

    def test_heading_navigation_rejects_invented_labels_and_missing_anchors(self):
        review = self.heading_review()
        entry = review['heading_navigation'][0]
        entry['label'] = 'Invented title'
        with self.assertRaisesRegex(ValueError, 'exactly retain source heading text'):
            merge.merge_epubs(self.paths, self.root / 'invented-heading.epub', 'A Work', review=review)
        entry['label'] = 'Chapter 1'
        entry['target_id'] = 'missing'
        with self.assertRaisesRegex(ValueError, 'unique heading anchor'):
            merge.merge_epubs(self.paths, self.root / 'missing-heading.epub', 'A Work', review=review)

    def test_heading_navigation_reuses_plain_div_heading_without_changing_markup(self):
        chapter = self.inputs[0]['OEBPS/chapter.xhtml'].replace(
            b'<h1 id="chapter">Chapter 1</h1>', b'<div class="book" id="chapter">BOOK IX.</div>')
        self.rewrite(self.paths[0], {'OEBPS/chapter.xhtml': chapter})
        review = self.heading_review()
        review['heading_navigation'][0]['label'] = 'BOOK IX.'
        report = merge.merge_epubs(self.paths, self.root / 'div-heading.epub', 'A Work', review=review)
        with zipfile.ZipFile(report['output']) as archive:
            self.assertEqual(archive.read('volumes/v001/OEBPS/chapter.xhtml'), chapter)
            nav = ET.fromstring(archive.read('nav.xhtml'))
            self.assertTrue(any(a.text == 'BOOK IX.' and a.get('href').endswith('#chapter')
                                for a in nav.iter(merge.tag(merge.XHTML, 'a'))))

    def test_heading_navigation_rejects_div_containing_a_section(self):
        chapter = self.inputs[0]['OEBPS/chapter.xhtml'].replace(
            b'<h1 id="chapter">Chapter 1</h1>', b'<div id="chapter"><p>Chapter 1</p></div>')
        self.rewrite(self.paths[0], {'OEBPS/chapter.xhtml': chapter})
        with self.assertRaisesRegex(ValueError, 'exactly retain source heading text'):
            merge.merge_epubs(self.paths, self.root / 'section-heading.epub', 'A Work', review=self.heading_review())

    def test_heading_navigation_rejects_wrong_source_and_nonreading_document(self):
        review = self.heading_review()
        entry = review['heading_navigation'][0]
        entry['source_sha256'] = 'wrong'
        with self.assertRaisesRegex(ValueError, 'unique source heading'):
            merge.merge_epubs(self.paths, self.root / 'wrong-heading-source.epub', 'A Work', review=review)
        entry['source_sha256'] = review['sources'][0]['sha256']
        entry.update(member='OEBPS/nav.xhtml', target_id='same-nav-id', label='Chapter 1')
        with self.assertRaisesRegex(ValueError, 'original reading-order XHTML'):
            merge.merge_epubs(self.paths, self.root / 'nav-heading-source.epub', 'A Work', review=review)

    def test_heading_navigation_rejects_reordered_headings(self):
        review = self.heading_review()
        review['heading_navigation'].insert(0, {
            'source_sha256': review['sources'][0]['sha256'], 'member': 'OEBPS/notes.xhtml',
            'target_id': 'note', 'label': 'Note 1 back', 'reason': 'Original notes heading.'})
        with self.assertRaisesRegex(ValueError, 'original reading order'):
            merge.merge_epubs(self.paths, self.root / 'reordered-headings.epub', 'A Work', review=review)

    def test_heading_navigation_rejects_duplicate_heading(self):
        review = self.heading_review()
        review['heading_navigation'] *= 2
        with self.assertRaisesRegex(ValueError, 'unique source heading'):
            merge.merge_epubs(self.paths, self.root / 'duplicate-heading.epub', 'A Work', review=review)

    def test_flattened_navigation_preserves_chapters_and_notes_with_colliding_names(self):
        report = self.run_merge()
        with zipfile.ZipFile(report['output']) as archive:
            self.assertEqual(archive.infolist()[0].filename, 'mimetype')
            self.assertEqual(archive.infolist()[0].compress_type, zipfile.ZIP_STORED)
            nav = ET.fromstring(archive.read('nav.xhtml'))
            toc = next(e for e in nav.iter(merge.tag(merge.XHTML, 'nav')) if e.get(merge.tag(merge.EPUB, 'type')) == 'toc')
            entries = toc.find(merge.tag(merge.XHTML, 'ol')).findall(merge.tag(merge.XHTML, 'li'))
            self.assertEqual([e.find(merge.tag(merge.XHTML, 'a')).text for e in entries], ['Chapter 1', 'Chapter 2'])
            self.assertTrue(all(e.find(merge.tag(merge.XHTML, 'ol')) is not None for e in entries))
            hrefs = [e.get('href') for e in toc.iter(merge.tag(merge.XHTML, 'a'))]
            self.assertEqual(hrefs, ['volumes/v001/OEBPS/chapter.xhtml#chapter', 'volumes/v001/OEBPS/notes.xhtml#note', 'volumes/v002/OEBPS/chapter.xhtml#chapter', 'volumes/v002/OEBPS/notes.xhtml#note'])
            ncx = ET.fromstring(archive.read('toc.ncx'))
            points = ncx.find(merge.tag(merge.NCX, 'navMap'))
            self.assertEqual([p.find(merge.tag(merge.NCX, 'navLabel')).find(merge.tag(merge.NCX, 'text')).text for p in points], ['Chapter 1', 'Chapter 2'])
            opf = ET.fromstring(archive.read('content.opf'))
            items = opf.find(merge.tag(merge.OPF, 'manifest'))
            self.assertEqual(sum('cover-image' in i.get('properties', '').split() for i in items), 1)
            self.assertEqual([i.get('idref') for i in opf.find(merge.tag(merge.OPF, 'spine'))], ['merged-nav', 'v001_chapter', 'v001_notes', 'v002_chapter', 'v002_notes'])
            sources = [e.text for e in opf.find(merge.tag(merge.OPF, 'metadata')).findall(merge.tag(merge.DC, 'source'))]
            self.assertEqual(sources, ['urn:volume:1', 'urn:volume:2'])
            for i, files in enumerate(self.inputs, 1):
                for name, data in files.items():
                    if not name.startswith('META-INF/') and name != 'OEBPS/content.opf':
                        self.assertEqual(archive.read(f'volumes/v{i:03}/{name}'), data)
        self.assertEqual([p.read_bytes() for p in self.paths], self.before)
        self.assertGreater(report['validation']['local_references_checked'], 10)

    def test_reproducible_output_for_the_same_inputs_and_timestamp(self):
        first = self.run_merge(self.root / 'a.epub')
        second = self.run_merge(self.root / 'b.epub')
        self.assertEqual(first['sha256'], second['sha256'])
        self.assertEqual(first['identifier'], second['identifier'])

    def test_existing_partial_owned_by_another_run_is_preserved(self):
        partial = self.root / 'combined.epub.partial'
        partial.write_bytes(b'another run owns this file')
        self.run_merge()
        self.assertEqual(partial.read_bytes(), b'another run owns this file')

    def test_metadata_identifiers_and_refinements_do_not_collide(self):
        with zipfile.ZipFile(self.paths[0]) as archive:
            files = {item.filename: archive.read(item.filename) for item in archive.infolist()}
        files['OEBPS/content.opf'] = files['OEBPS/content.opf'].replace(b'id="author"', b'id="merged-id"').replace(b'refines="#author"', b'refines="#merged-id"')
        with zipfile.ZipFile(self.paths[0], 'w') as archive:
            for name, data in files.items():
                archive.writestr(name, data)
        report = self.run_merge()
        with zipfile.ZipFile(report['output']) as archive:
            package = ET.fromstring(archive.read('content.opf'))
        ids = [element.get('id') for element in package.iter() if element.get('id')]
        self.assertEqual(len(ids), len(set(ids)))
        metadata = package.find(merge.tag(merge.OPF, 'metadata'))
        creator = metadata.find(merge.tag(merge.DC, 'creator'))
        role = next(e for e in metadata if e.get('property') == 'role')
        self.assertEqual(role.get('refines'), '#' + creator.get('id'))

    def test_generic_numbered_sections_stay_inside_their_chapter(self):
        volume = merge.read_volume(self.paths[0], 'Volume 1', 1)
        volume.toc = [{'label': 'A Book', 'href': volume.prefix + 'OEBPS/chapter.xhtml#book', 'children': [
            {'label': 'Chapter One', 'href': volume.prefix + 'OEBPS/chapter.xhtml#chapter-one', 'children': []},
            {'label': 'Chapter Two', 'href': volume.prefix + 'OEBPS/chapter.xhtml#chapter-two', 'children': []}]}]
        volume.files['OEBPS/chapter.xhtml'] = '<html xmlns="http://www.w3.org/1999/xhtml"><head><title>A Book</title></head><body><h1 id="book">A Book</h1><h2 id="chapter-one">Chapter One</h2><p>§ 1. First.</p><p>§ 2. Second.</p><h2 id="chapter-two">Chapter Two</h2><p>§ 3. Third.</p></body></html>'.encode()
        merge.expand_numbered_sections(volume)
        children = volume.toc[0]['children']
        self.assertEqual([e['label'] for e in children], ['Chapter One', 'Chapter Two'])
        self.assertEqual([e['label'] for e in children[0]['children']], ['§ 1', '§ 2'])
        self.assertEqual([e['label'] for e in children[1]['children']], ['§ 3'])

    def set_modified(self, path, value, refinement=None):
        with zipfile.ZipFile(path) as archive:
            files = {item.filename: archive.read(item.filename) for item in archive.infolist()}
        data = files['OEBPS/content.opf'].replace(b'2026-01-01T00:00:00Z', value.encode())
        if refinement:
            data = data.replace(b'</metadata>', ('<meta property="dcterms:modified" refines="#author">' + refinement + '</meta></metadata>').encode())
        files['OEBPS/content.opf'] = data
        with zipfile.ZipFile(path, 'w') as archive:
            for name, data in files.items():
                archive.writestr(name, data)

    def test_default_timestamp_is_first_source_value_and_rebuilds_are_identical(self):
        self.set_modified(self.paths[0], '2025-03-04T05:06:07Z')
        self.set_modified(self.paths[1], '2026-10-12T11:12:13Z')
        first = merge.merge_epubs(self.paths, self.root / 'first.epub', 'A Work')
        second = merge.merge_epubs(self.paths, self.root / 'second.epub', 'A Work')
        self.assertEqual(first['modified'], '2025-03-04T05:06:07Z')
        self.assertEqual(first['sha256'], second['sha256'])
        self.assertEqual(first['identifier'], second['identifier'])
        with zipfile.ZipFile(first['output']) as archive:
            package = ET.fromstring(archive.read('content.opf'))
            dates = [e.text for e in package.find(merge.tag(merge.OPF, 'metadata')) if e.get('property') == 'dcterms:modified']
            self.assertEqual(dates, ['2025-03-04T05:06:07Z'])

    def test_invalid_and_refined_source_dates_are_skipped(self):
        self.set_modified(self.paths[0], '2026-02-31T00:00:00Z', refinement='2025-01-01T00:00:00Z')
        self.set_modified(self.paths[1], '2024-02-29T12:34:56Z')
        result = merge.merge_epubs(self.paths, self.root / 'valid.epub', 'A Work')
        self.assertEqual(result['modified'], '2024-02-29T12:34:56Z')

    def test_missing_valid_source_date_requires_explicit_value(self):
        for path in self.paths:
            self.set_modified(path, 'not-a-date')
        with self.assertRaisesRegex(ValueError, 'No valid source last-modified'):
            merge.merge_epubs(self.paths, self.root / 'no-date.epub', 'A Work')
        self.assertFalse((self.root / 'no-date.epub').exists())
        result = merge.merge_epubs(self.paths, self.root / 'override.epub', 'A Work', modified='2026-10-12T00:00:00Z')
        self.assertEqual(result['modified'], '2026-10-12T00:00:00Z')

    def test_explicit_timestamp_changes_revision_identity(self):
        first = merge.merge_epubs(self.paths, self.root / 'a.epub', 'A Work', modified='2026-01-01T00:00:00Z')
        second = merge.merge_epubs(self.paths, self.root / 'b.epub', 'A Work', modified='2026-01-01T00:00:01Z')
        self.assertNotEqual(first['sha256'], second['sha256'])
        self.assertNotEqual(first['identifier'], second['identifier'])

    def test_incompatible_languages_are_rejected_without_output(self):
        fixture(self.paths[1], 2, language='sv')
        with self.assertRaisesRegex(ValueError, 'same credited authors and languages'):
            self.run_merge()
        self.assertFalse((self.root / 'combined.epub').exists())

    def test_duplicate_volume_is_rejected(self):
        with self.assertRaisesRegex(ValueError, 'same volume'):
            merge.merge_epubs([self.paths[0], self.paths[0]], self.root / 'combined.epub', 'Duplicate')

    def test_different_translation_credits_are_rejected(self):
        with zipfile.ZipFile(self.paths[1]) as archive:
            files = {item.filename: archive.read(item.filename) for item in archive.infolist()}
        files['OEBPS/content.opf'] = files['OEBPS/content.opf'].replace(b'</metadata>', b'<dc:contributor>A Different Translator</dc:contributor></metadata>')
        with zipfile.ZipFile(self.paths[1], 'w') as archive:
            for name, data in files.items():
                archive.writestr(name, data)
        with self.assertRaisesRegex(ValueError, 'translators/contributors'):
            self.run_merge()

    def test_encrypted_resources_and_unsupported_versions_are_rejected(self):
        for options, message in [({'encrypted': True}, 'encrypted or signed'), ({'version': '1.0'}, 'Only EPUB2 and EPUB3')]:
            with self.subTest(options=options):
                fixture(self.paths[1], 2, **options)
                with self.assertRaisesRegex(ValueError, message):
                    self.run_merge()

    def legacy_inputs(self, target='chapter.xhtml#chapter'):
        originals = []
        for number, path in enumerate(self.paths, 1):
            files = fixture(path, number, version='2.0')
            package = ET.fromstring(files['OEBPS/content.opf'])
            metadata = package.find(merge.tag(merge.OPF, 'metadata'))
            for element in list(metadata):
                if element.tag == merge.tag(merge.OPF, 'meta'):
                    metadata.remove(element)
            author = metadata.find(merge.tag(merge.DC, 'creator'))
            author.set(merge.tag(merge.OPF, 'role'), 'aut')
            author.set(merge.tag(merge.OPF, 'file-as'), 'Author, An')
            ET.SubElement(metadata, merge.tag(merge.OPF, 'meta'),
                          {'name': 'calibre:timestamp', 'content': '2020-04-05T12:13:14.123+02:00'})
            ET.SubElement(metadata, merge.tag(merge.OPF, 'meta'), {'name': 'cover', 'content': 'cover'})
            manifest = package.find(merge.tag(merge.OPF, 'manifest'))
            for item in list(manifest):
                item.attrib.pop('properties', None)
                if item.get('id') == 'nav':
                    manifest.remove(item)
            ET.SubElement(manifest, merge.tag(merge.OPF, 'item'),
                          {'id': 'ncx', 'href': 'toc.ncx', 'media-type': 'application/x-dtbncx+xml'})
            package.find(merge.tag(merge.OPF, 'spine')).set('toc', 'ncx')
            files['OEBPS/content.opf'] = ET.tostring(package)
            files.pop('OEBPS/nav.xhtml')
            files['OEBPS/toc.ncx'] = f'''<ncx xmlns="{merge.NCX}" version="2005-1"><navMap>
              <navPoint id="chapter"><navLabel><text>Original chapter {number}</text></navLabel><content src="{target}"/>
                <navPoint id="notes"><navLabel><text>Original notes {number}</text></navLabel><content src="notes.xhtml#note"/></navPoint>
              </navPoint></navMap><pageList><pageTarget id="page"><navLabel><text>i</text></navLabel><content src="chapter.xhtml#chapter"/></pageTarget></pageList></ncx>'''.encode()
            with zipfile.ZipFile(path, 'w') as archive:
                archive.writestr('mimetype', b'application/epub+zip')
                for name, data in files.items():
                    archive.writestr(name, data)
            originals.append(files)
        return originals

    def test_epub2_conversion_preserves_original_ncx_labels_content_credits_and_timestamp(self):
        originals = self.legacy_inputs()
        before = [p.read_bytes() for p in self.paths]
        report = merge.merge_epubs(self.paths, self.root / 'legacy.epub', 'A Work')
        self.assertEqual(report['modified'], '2020-04-05T10:13:14Z')
        self.assertEqual([p.read_bytes() for p in self.paths], before)
        with zipfile.ZipFile(report['output']) as archive:
            nav = ET.fromstring(archive.read('nav.xhtml'))
            toc = next(n for n in nav.iter(merge.tag(merge.XHTML, 'nav')) if n.get(merge.tag(merge.EPUB, 'type')) == 'toc')
            entries = merge.navigation_entries(toc.find(merge.tag(merge.XHTML, 'ol')), 'nav.xhtml', '')
            self.assertEqual([n['label'] for n in entries], ['Original chapter 1', 'Original chapter 2'])
            self.assertEqual(entries[0]['children'][0]['label'], 'Original notes 1')
            package = ET.fromstring(archive.read('content.opf'))
            self.assertEqual(package.get('version'), '3.0')
            metadata = package.find(merge.tag(merge.OPF, 'metadata'))
            self.assertTrue(any(e.get('property') == 'role' and e.text == 'aut' for e in metadata))
            self.assertTrue(any(e.get('property') == 'file-as' and e.text == 'Author, An' for e in metadata))
            self.assertEqual(sum('cover-image' in e.get('properties', '').split() for e in package.find(merge.tag(merge.OPF, 'manifest'))), 1)
            for number, files in enumerate(originals, 1):
                for name, data in files.items():
                    if name.endswith(('.xhtml', '.css', '.png')):
                        self.assertEqual(archive.read(f'volumes/v{number:03}/{name}'), data)
        self.assertEqual(report['volumes'][0]['navigation_source'], 'original_ncx')

    def test_epub2_requires_original_ncx_and_local_targets(self):
        fixture(self.paths[0], 1, version='2.0')
        with self.assertRaisesRegex(ValueError, 'unambiguous original NCX'):
            merge.read_volume(self.paths[0], '1', 1)
        for target, message in [('https://example.org/book', 'must be local'), ('../../outside.xhtml', 'escapes'), ('', 'original label and local target')]:
            self.legacy_inputs(target)
            with self.subTest(target=target), self.assertRaisesRegex(ValueError, message):
                merge.read_volume(self.paths[0], '1', 1)

    def test_epub2_without_intrinsic_timestamp_does_not_invent_one(self):
        originals = self.legacy_inputs()
        for path, files in zip(self.paths, originals):
            package = ET.fromstring(files['OEBPS/content.opf'])
            metadata = package.find(merge.tag(merge.OPF, 'metadata'))
            for element in list(metadata):
                if element.get('name') == 'calibre:timestamp':
                    metadata.remove(element)
            ET.SubElement(metadata, merge.tag(merge.DC, 'date')).text = '1900'
            self.rewrite(path, {'OEBPS/content.opf': ET.tostring(package)})
        with self.assertRaisesRegex(ValueError, 'No valid source last-modified'):
            merge.merge_epubs(self.paths, self.root / 'undated.epub', 'A Work')

    def test_broken_footnote_target_prevents_publication(self):
        fixture(self.paths[1], 2, broken=True)
        with self.assertRaisesRegex(ValueError, 'Missing fragment target'):
            self.run_merge()
        self.assertFalse((self.root / 'combined.epub').exists())
        self.assertFalse((self.root / 'combined.epub.partial').exists())

    def test_sources_and_existing_output_are_never_overwritten(self):
        with self.assertRaisesRegex(ValueError, 'new file'):
            self.run_merge(self.paths[0])
        output = self.root / 'combined.epub'
        output.write_bytes(b'existing')
        with self.assertRaisesRegex(ValueError, 'new file'):
            self.run_merge(output)
        self.assertEqual(output.read_bytes(), b'existing')
        self.assertEqual([p.read_bytes() for p in self.paths], self.before)

    def test_traversal_and_root_paths_are_rejected(self):
        for value in ['../../../../outside.png', '/OEBPS/chapter.xhtml']:
            with self.subTest(value=value), self.assertRaises(ValueError):
                merge.local_target('OEBPS/nav.xhtml', value)

    def test_explicit_numbered_sections_get_anchors_without_changing_text_or_links(self):
        with zipfile.ZipFile(self.paths[0]) as archive:
            files = {item.filename: archive.read(item.filename) for item in archive.infolist()}
        original = files['OEBPS/chapter.xhtml'].replace(b'</body>',
            '<p style=\'content: ">"\'>§ 1. First <em>section</em>.</p><p id="existing-section">§ 2. Second section.</p><p>§ 3. Third section.</p></body>'.encode())
        files['OEBPS/chapter.xhtml'] = original
        with zipfile.ZipFile(self.paths[0], 'w') as archive:
            for name, data in files.items():
                archive.writestr(name, data)
        source_before = self.paths[0].read_bytes()
        report = merge.merge_epubs(self.paths, self.root / 'sections.epub', 'Numbered Sections', expand_sections=True)
        with zipfile.ZipFile(report['output']) as archive:
            data = archive.read('volumes/v001/OEBPS/chapter.xhtml')
            expected = original.replace(b'<p style=\'content: ">"\'>', b'<p style=\'content: ">"\' id="bokheim-section-1">').replace('<p>§ 3.'.encode(), '<p id="bokheim-section-3">§ 3.'.encode())
            self.assertEqual(data, expected)
            self.assertEqual(''.join(ET.fromstring(data).itertext()), ''.join(ET.fromstring(original).itertext()))
            nav = ET.fromstring(archive.read('nav.xhtml'))
            labels = [e.text for e in nav.iter(merge.tag(merge.XHTML, 'a'))]
            self.assertTrue(all(label in labels for label in ['§ 1', '§ 2', '§ 3']))
            self.assertEqual(archive.read('volumes/v002/OEBPS/chapter.xhtml'), self.inputs[1]['OEBPS/chapter.xhtml'])
        self.assertEqual(len(report['volumes'][0]['added_numbered_sections']), 3)
        self.assertFalse(report['content_preserved'])
        self.assertTrue(report['visible_text_preserved'])
        self.assertEqual(self.paths[0].read_bytes(), source_before)

    def test_quoted_section_references_inside_paragraphs_are_not_added(self):
        volume = merge.read_volume(self.paths[0], 'Volume 1', 1)
        volume.files['OEBPS/chapter.xhtml'] = volume.files['OEBPS/chapter.xhtml'].replace(b'</body>',
            '<p>As discussed in § 1. this is a reference.</p><p>“§ 2. A quoted section.”</p><p>The argument refers to § 3.</p></body>'.encode())
        modified, entries = merge.expand_numbered_sections(volume)
        self.assertEqual(modified, {})
        self.assertEqual(entries, [])


if __name__ == '__main__':
    unittest.main()
