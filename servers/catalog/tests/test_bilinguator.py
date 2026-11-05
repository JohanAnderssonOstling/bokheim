import importlib.util
import io
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
import xml.etree.ElementTree as ET
import zipfile

SPEC = importlib.util.spec_from_file_location('bilinguator', Path(__file__).parents[1] / 'tools/ingest-bilinguator.py')
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class BilinguatorTests(unittest.TestCase):
    def test_expiry_requires_seventy_complete_years(self):
        notice = dict(text='The translation is/are in the public domain and not protected by copyright because 50 years have passed since the author’s death in 1972.')
        self.assertIsNone(MODULE.license_for(notice))
        notice['text'] = notice['text'].replace('1972', str(MODULE.datetime.now(MODULE.timezone.utc).year - 70))
        self.assertIsNone(MODULE.license_for(notice))
        notice['text'] = notice['text'].replace(str(MODULE.datetime.now(MODULE.timezone.utc).year - 70), '1901')
        self.assertEqual(MODULE.license_for(notice)['id'], 'public-domain')

    def test_license_versions_and_noncommercial_terms_are_retained(self):
        for name, slug, version in [('CC BY-NC 4.0', 'by-nc', '4.0'), ('CC BY-SA 3.0', 'by-sa', '3.0'), ('CC BY-NC-SA 3.0', 'by-nc-sa', '3.0')]:
            url = f'https://creativecommons.org/licenses/{slug}/{version}/'
            notice = dict(text=f'The translation is/are distributed under the {name} license.', links=[url])
            self.assertEqual(MODULE.license_for(notice), dict(id=name.replace(' ', '-'), url=url))
            notice['links'] = ['https://example.com/']
            self.assertIsNone(MODULE.license_for(notice))

    def test_unknown_language_rights_hold_only_affected_pairs(self):
        notice = dict(text='The text is/are distributed under the CC0 license.')
        works = [dict(id='work', title='Title', url='https://bilinguator.com/en/book?book=work', languages=dict(en='English', fr='French', de='German'), rights=dict(en=notice, fr=notice))]
        selected, held = MODULE.selection(works)
        self.assertEqual([row['languages'] for row in selected], [['en', 'fr']])
        self.assertEqual(len(held), 2)

    def test_esperanto_subject_exclusion_applies_to_pairs_without_esperanto(self):
        notice = dict(text='The text is/are distributed under the CC0 license.')
        works = [dict(id=identity, title=title, url=f'{MODULE.ORIGIN}/en/book?book={identity}',
                      languages=dict(en='English', de='German', fr='French'),
                      rights=dict(en=notice, de=notice, fr=notice))
                 for identity, title in [('manifesto_de_prago', 'Prague Manifesto'),
                                         ('manifesto', 'Manifesto of the Communist Party'),
                                         ('alice_in_wonderland', 'Alice’s adventures in Wonderland')]]
        selected, held = MODULE.selection(works)
        self.assertEqual({row['work_id'] for row in selected}, {'manifesto', 'alice_in_wonderland'})
        self.assertEqual(len(selected), 6)
        self.assertEqual(len(held), 3)
        self.assertTrue(all(row['work_id'] == 'manifesto_de_prago' and 'about Esperanto' in row['reason'] for row in held))
        self.assertIn(['en', 'fr'], [row['languages'] for row in held])

    def test_esperanto_language_exclusion_keeps_other_pairings_of_the_same_work(self):
        notice = dict(text='The text is/are distributed under the CC0 license.')
        languages = {'en': 'English', 'fr': 'French', 'eo': 'Esperanto',
                     'epo': 'Esperanto', 'EO_latn': 'Esperanto'}
        work = dict(id='alice_in_wonderland', title='Alice’s adventures in Wonderland',
                    url=f'{MODULE.ORIGIN}/en/book?book=alice_in_wonderland',
                    languages=languages, rights=dict.fromkeys(languages, notice))
        selected, held = MODULE.selection([work])
        self.assertEqual([row['languages'] for row in selected], [['en', 'fr']])
        self.assertEqual(len(held), 9)
        self.assertTrue(all('containing Esperanto' in row['reason'] for row in held))

    def test_unsupported_language_exclusion_matches_aliases_and_preserves_supported_pairs(self):
        notice = dict(text='The text is/are distributed under the CC0 license.')
        languages = {'en': 'English', 'sv': 'Swedish', 'fr-CA': 'French',
                     'HR_hr': 'Croatian', 'hrv': 'Croatian'}
        work = dict(id='work', title='Title', url=f'{MODULE.ORIGIN}/en/book?book=work',
                    languages=languages, rights=dict.fromkeys(languages, notice))
        selected, held = MODULE.selection([work])
        self.assertEqual({tuple(row['languages']) for row in selected},
                         {('en', 'sv'), ('en', 'fr-CA'), ('fr-CA', 'sv')})
        self.assertEqual(len(held), 7)
        self.assertTrue(all('unsupported language: hr' in row['reason'] for row in held))

    def epub(self):
        output = io.BytesIO()
        with zipfile.ZipFile(output, 'w') as archive:
            archive.writestr('mimetype', 'application/epub+zip', compress_type=zipfile.ZIP_STORED)
            archive.writestr('META-INF/container.xml', '<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OPS/content.opf"/></rootfiles></container>')
            archive.writestr('OPS/content.opf', f'<package xmlns="{MODULE.OPF}"><metadata xmlns:dc="{MODULE.DC}"><dc:title>A &amp; B / C</dc:title><dc:creator>Author / Auteur</dc:creator><dc:identifier>work_en_fr</dc:identifier><dc:language>en</dc:language><dc:language>fr</dc:language></metadata><manifest><item id="text" href="text.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="text"/></spine></package>')
            archive.writestr('OPS/text.xhtml', '<html xmlns="http://www.w3.org/1999/xhtml"><body>Original &amp; translated text.</body></html>')
            archive.writestr('OPS/illustration.png', b'original-image')
        return output.getvalue()

    def test_rights_travel_with_download_without_changing_original_resources(self):
        original = self.epub()
        notices = [dict(text='The English translation authored by Alice & Bob is/are distributed under the CC BY-NC 4.0 license.', url='https://bilinguator.com/en/bilingual?book=1'), dict(text='The French translation authored by René is/are distributed under the CC0 license.', url='https://bilinguator.com/en/bilingual?book=1')]
        # Use the source notice grammar to verify translator metadata as well.
        notices[0]['text'] = notices[0]['text'].replace('translation authored', 'translation of the book “Title” authored')
        notices[1]['text'] = notices[1]['text'].replace('translation authored', 'translation of the book “Title” authored')
        row = dict(entry_id='work_en_fr', languages=['en', 'fr'], notices=notices,
                   licenses=[dict(id='CC-BY-NC-4.0', url='https://creativecommons.org/licenses/by-nc/4.0/'), dict(id='CC0-1.0', url='https://creativecommons.org/publicdomain/zero/1.0/')])
        derivative, receipt = MODULE.attributed_epub(original, row, dict(author='Author', url='https://bilinguator.com/en/book?book=work'), {'CC-BY-NC-4.0': 'Noncommercial legal text', 'CC0-1.0': 'CC0 legal text'})
        self.assertEqual(receipt['translators'], ['Alice & Bob', 'René'])
        with zipfile.ZipFile(io.BytesIO(original)) as before, zipfile.ZipFile(io.BytesIO(derivative)) as after:
            for name in before.namelist():
                if name != 'OPS/content.opf':
                    self.assertEqual(before.read(name), after.read(name))
            self.assertEqual(after.infolist()[0].filename, 'mimetype')
            self.assertEqual(after.infolist()[0].compress_type, zipfile.ZIP_STORED)
            credits = after.read('OPS/bokheim-rights.xhtml')
            ET.fromstring(credits)
            self.assertIn(b'Noncommercial legal text', credits)
            self.assertIn(b'Alice &amp; Bob', credits)
            root = ET.fromstring(after.read('OPS/content.opf'))
            self.assertEqual(root.find(f'{{{MODULE.OPF}}}metadata/{{{MODULE.DC}}}creator').text, 'Author')
            self.assertEqual(root.find(f'{{{MODULE.OPF}}}spine')[-1].get('idref'), 'bokheim-rights')

    def test_wrong_language_or_work_downloads_are_rejected(self):
        for identity, languages in [('other_en_fr', ['en', 'fr']), ('work_en_fr', ['en', 'sv'])]:
            with self.assertRaises(ValueError):
                MODULE.package_info(self.epub(), identity, languages)

    def test_individual_expiry_review_is_bound_to_the_named_source_text(self):
        data = b'Original author\n<h1>Title</h1><delimiter>Translated by A Translator.\nBody\n'
        review = dict(work_id='work', language='en', author='A Translator', death_year=1901,
                      kind='translation', source_text_url='https://bilinguator.com/texts/work/work_en.txt',
                      source_text_sha256='wrong', authority_urls=['https://library.example/authority'])
        works = [dict(id='work', title='Title', languages={'en': 'English'}, rights={})]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'review.json'
            path.write_text(json.dumps([review]))
            with self.assertRaises(ValueError):
                MODULE.apply_rights_reviews(SimpleNamespace(fetch=lambda _: data), works, path)
            review['source_text_sha256'] = MODULE.digest(data)
            path.write_text(json.dumps([review]))
            result = MODULE.apply_rights_reviews(SimpleNamespace(fetch=lambda _: data), works, path)
        notice = result[0]['rights']['en']
        self.assertEqual(MODULE.license_for(notice)['id'], 'public-domain')
        self.assertIn('not a Bilinguator license declaration', notice['text'])


if __name__ == '__main__':
    unittest.main()
