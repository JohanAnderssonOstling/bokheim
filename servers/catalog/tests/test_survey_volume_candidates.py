"""Regressions for complete sets hidden by title punctuation or credit changes."""
import importlib.util
from pathlib import Path
import unittest


SPEC = importlib.util.spec_from_file_location('survey_volume_candidates', Path(__file__).parents[1] / 'tools/survey-volume-candidates.py')
survey = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(survey)


class CandidateTests(unittest.TestCase):
    def test_swedish_ordinal_parts_require_extent_review(self):
        for marker, expected in [('Första delen', 1), ('Andra delen', 2), ('Fjerde delen', 4), ('Senare delen', 2)]:
            parsed = survey.parse_title('Ett verk. ' + marker)
            self.assertEqual((parsed['base_title'], parsed['number'], parsed['stated_total']), ('Ett verk', expected, None))

    def test_bare_swedish_numbers_are_opt_in_candidates_without_implied_extent(self):
        self.assertIsNone(survey.parse_title('Barnets århundrade. II'))
        parsed = survey.parse_title('Barnets århundrade. II', allow_bare_number=True)
        self.assertEqual((parsed['base_title'], parsed['number'], parsed['stated_total']), ('Barnets århundrade', 2, None))
        self.assertIn('membership_review', parsed['numbering_evidence'])
        self.assertIsNone(survey.parse_title('Dikter 1900', allow_bare_number=True))

    def test_swedish_compound_number_is_only_a_part_of_its_parent_volume(self):
        parsed = survey.parse_title('Svenska siare och skalder VI:2', allow_bare_number=True)
        self.assertEqual((parsed['base_title'], parsed['number'], parsed['scope']),
                         ('Svenska siare och skalder', 6, 'excerpt'))
        self.assertEqual(parsed['components'], [{'kind': 'part', 'number': 2, 'stated_total': None}])

    def entry(self, title, contributors=(), language='en', creator='An Author'):
        return {'hash': title, 'title': title, 'creators': [creator],
                'contributors': list(contributors), 'languages': [language],
                **survey.parse_title(title)}

    def test_paine_extent_after_period(self):
        parsed = survey.parse_title('The Life Of Thomas Paine, Vol. 1. (of 2) / With A History')
        self.assertEqual((parsed['number'], parsed['stated_total']), (1, 2))

    def test_intro_credit_does_not_hide_gibbon_pair(self):
        groups = survey.group_candidates([
            self.entry('Private Letters of Edward Gibbon (1753-1794) Volume 1 (of 2)', ['Editor', 'Introduction']),
            self.entry('Private Letters of Edward Gibbon (1753-1794) Volume 2 (of 2)', ['Editor']),
        ])
        self.assertEqual(len(groups), 1)
        self.assertTrue(groups[0]['credit_variation'])
        self.assertEqual(groups[0]['status'], 'contiguous_stated_range_needs_edition_review')

    def test_creator_variation_still_requires_review(self):
        groups = survey.group_candidates([self.entry('A Work, Volume 1 (of 2)'),
                                          self.entry('A Work, Volume 2 (of 2)', creator='Another Author')])
        self.assertTrue(groups[0]['credit_variation'])
        self.assertIn('review', groups[0]['status'])

    def test_languages_remain_separate(self):
        groups = survey.group_candidates([self.entry('A Work, Volume 1 (of 2)'),
                                          self.entry('A Work, Volume 2 (of 2)', language='fr')])
        self.assertEqual(len(groups), 2)
        self.assertTrue(all(g['status'] == 'missing_volumes' for g in groups))

    def test_bundled_work_is_not_a_single_volume(self):
        parsed = survey.parse_title('A Work, Volumes I and II')
        self.assertEqual(parsed['scope'], 'already_bundled')
        self.assertEqual(survey.group_candidates([self.entry('A Work, Volumes I and II')]), [])

    def test_repeated_volume_word_in_bundled_title(self):
        self.assertEqual(survey.parse_title('A Work, Volume I and Volume II')['scope'], 'already_bundled')
        self.assertEqual(survey.group_candidates([self.entry('A Work, Volume I and Volume II')]), [])

    def test_fragments_are_not_complete_volumes(self):
        self.assertEqual(survey.parse_title("Holinshed's Chronicles, vol. 1 of 6, slice 9 of 9")['scope'], 'excerpt')

    def test_complete_slices_reconstruct_only_their_parent_volume(self):
        entries = [self.entry(f'A Chronicle, vol. 2 of 6, slice {n} of 3') for n in range(1, 4)]
        self.assertEqual(survey.group_candidates(entries), [])
        group = survey.group_component_candidates(entries)[0]
        self.assertEqual((group['parent_number'], group['numbers'], group['stated_totals']), (2, [1, 2, 3], [3]))
        self.assertEqual(group['status'], 'contiguous_component_range_needs_edition_review')
        self.assertTrue(all(v['stated_total'] == 6 for v in group['volumes']))

    def test_combined_essays_are_counted_without_inventing_extent(self):
        entries = [self.entry('Miscellanies, Vol. 3 of 3, Essay 1: Culture'),
                   self.entry('Miscellanies, Vol. 3 of 3, Essay 2: Mill; Essay 3: Autobiography')]
        group = survey.group_component_candidates(entries)[0]
        self.assertEqual(group['numbers'], [1, 2, 3])
        self.assertEqual(group['stated_totals'], [])
        self.assertEqual(group['status'], 'component_extent_needs_review')

    def test_component_gaps_and_overlapping_files_require_review(self):
        first = self.entry('A Chronicle, vol. 2 of 6, slice 1 of 3')
        last = self.entry('A Chronicle, vol. 2 of 6, slice 3 of 3')
        self.assertEqual(survey.group_component_candidates([first, last])[0]['status'], 'missing_components')
        duplicate = self.entry('A Chronicle, vol. 2 of 6, slice 03 of 3')
        self.assertEqual(survey.group_component_candidates([first, last, duplicate])[0]['status'],
                         'duplicate_components_need_review')

    def test_component_groups_keep_parent_volumes_and_languages_separate_and_review_credits(self):
        entries = [self.entry('A Chronicle, vol. 1 of 6, slice 1 of 3'),
                   self.entry('A Chronicle, vol. 2 of 6, slice 2 of 3'),
                   self.entry('A Chronicle, vol. 1 of 6, slice 2 of 3', language='fr'),
                   self.entry('A Chronicle, vol. 1 of 6, slice 3 of 3', creator='Another Author')]
        groups = survey.group_component_candidates(entries)
        self.assertEqual(len(groups), 3)
        self.assertEqual(sum(g['credit_variation'] for g in groups), 1)

    def test_extra_geography_author_does_not_hide_complete_holinshed_slices(self):
        entries = [self.entry('A Chronicle, vol. 1 of 6, slice 1 of 2', creator='Holinshed and Harrison'),
                   self.entry('A Chronicle, vol. 1 of 6, slice 2 of 2', creator='Holinshed')]
        group = survey.group_component_candidates(entries)[0]
        self.assertTrue(group['credit_variation'])
        self.assertEqual(group['status'], 'contiguous_component_range_needs_edition_review')

    def test_conflicting_component_totals_are_not_complete(self):
        entries = [self.entry('A Chronicle, vol. 2 of 6, slice 1 of 2'),
                   self.entry('A Chronicle, vol. 2 of 6, slice 2 of 3')]
        self.assertEqual(survey.group_component_candidates(entries)[0]['status'],
                         'conflicting_component_extent_needs_review')

    def test_word_number_and_french_ordinals(self):
        self.assertEqual(survey.parse_title('A Tour, Volume Two')['number'], 2)
        self.assertEqual(survey.parse_title('De la Démocratie en Amérique, tome premier')['number'], 1)
        self.assertEqual(survey.parse_title('De la Démocratie en Amérique, tome deuxième')['number'], 2)

    def test_contiguous_numbers_do_not_prove_extent(self):
        groups = survey.group_candidates([self.entry('A Work, Volume I'), self.entry('A Work, Volume II')])
        self.assertEqual(groups[0]['status'], 'extent_needs_review')

    def test_german_number_before_band(self):
        for marker, expected in [('I. Band', 1), ('Erster Band', 1), ('Zweiter Band', 2)]:
            parsed = survey.parse_title('Eine Geschichte, ' + marker)
            self.assertEqual((parsed['base_title'], parsed['number']), ('Eine Geschichte', expected))

    def test_volume_without_space(self):
        self.assertEqual(survey.parse_title('Monsieur Lecoq — Volume2')['number'], 2)

    def test_extent_statement_is_not_prefixed_volume_marker(self):
        parsed = survey.parse_title('A History in Three Volumes, Vol. II')
        self.assertEqual(parsed['number'], 2)

    def test_duplicate_numbers_are_flagged(self):
        groups = survey.group_candidates([self.entry('A Work, Volume I (of 2)'), self.entry('A Work, Vol. I (of 2)')])
        self.assertEqual(groups[0]['status'], 'duplicate_numbers_need_review')

    def test_warren_hearings_share_a_title_despite_collection_positions(self):
        groups = survey.group_candidates([
            self.entry('Warren Commission (02 of 26): Hearings Vol. II (of 15)'),
            self.entry('Warren Commission (05 of 26): Hearings Vol. V (of 15)'),
            self.entry('Warren Commission (15 of 26): Hearings Vol. XV (of 15)'),
        ])
        self.assertEqual(len(groups), 1)
        self.assertEqual(groups[0]['title'], 'Warren Commission: Hearings')
        self.assertEqual(groups[0]['numbers'], [2, 5, 15])
        self.assertEqual(groups[0]['stated_totals'], [15])
        self.assertEqual(groups[0]['status'], 'missing_volumes')

    def test_warren_exhibits_and_other_named_subseries_remain_separate(self):
        parsed = survey.parse_title('Warren Commission (16 of 26): Exhibits Vol. I (of 11)')
        self.assertEqual(parsed['base_title'], 'Warren Commission (16 of 26): Exhibits')
        parsed = survey.parse_title('A Collection (02 of 26): Essays Vol. II (of 15)')
        self.assertEqual(parsed['base_title'], 'A Collection (02 of 26): Essays')

    def test_slash_extent_and_michelet_coverage_dates_do_not_split_the_work(self):
        groups = survey.group_candidates([
            self.entry('Histoire de France (Volume 1/19)', language='fr', creator='Jules Michelet'),
            self.entry('Histoire de France 814-1189 (Volume 2/19)', language='fr', creator='Jules Michelet'),
            self.entry('Histoire de France 1724-1759 (Volume 18/19)', language='fr', creator='Jules Michelet'),
        ])
        self.assertEqual(len(groups), 1)
        self.assertEqual(groups[0]['title'], 'Histoire de France')
        self.assertEqual(groups[0]['numbers'], [1, 2, 18])
        self.assertEqual(groups[0]['stated_totals'], [19])
        self.assertEqual(groups[0]['status'], 'missing_volumes')

    def test_separately_edited_michelet_medieval_history_stays_separate(self):
        groups = survey.group_candidates([
            self.entry('Histoire de France 814-1189 (Volume 2/19)', language='fr'),
            self.entry('Histoire de France - Moyen Âge; (Vol. 2 / 10)', ['Gabriel Monod'], language='fr'),
        ])
        self.assertEqual(len(groups), 2)
        self.assertEqual(sorted(g['stated_totals'][0] for g in groups), [10, 19])

    def test_english_ordinal_parts_are_discovered_without_claiming_extent(self):
        groups = survey.group_candidates([
            self.entry('The Heroic Enthusiasts (Gli Eroici Furori) Part the First / An Ethical Poem'),
            self.entry('The Heroic Enthusiasts (Gli Eroici Furori) Part the Second / An Ethical Poem'),
        ])
        self.assertEqual(len(groups), 1)
        self.assertEqual(groups[0]['numbers'], [1, 2])
        self.assertEqual(groups[0]['status'], 'extent_needs_review')
        self.assertEqual(survey.parse_title('A Work, Part Third')['number'], 3)

    def test_descriptive_part_words_do_not_become_numbered_parts(self):
        self.assertIsNone(survey.parse_title('The Part Borne by the Dutch in the Discovery of Australia 1606-1765'))
        self.assertIsNone(survey.parse_title('Novum organon renovatum / Being the second part of the philosophy of the inductive sciences'))

    def test_book_ranges_are_discovered_without_claiming_completeness(self):
        groups = survey.group_candidates([
            self.entry('The History of Rome, Books 01 to 08'),
            self.entry('The History of Rome, Books 09 to 26'),
            self.entry('The History of Rome, Books 27 to 36'),
            self.entry('The History of Rome, Books 37 to the End / with the Epitomes and Fragments'),
        ])
        self.assertEqual(len(groups), 1)
        self.assertEqual(groups[0]['numbers'], [1, 9, 27, 37])
        self.assertEqual([v['range_end'] for v in groups[0]['volumes']], [8, 26, 36, None])
        self.assertEqual(groups[0]['status'], 'book_ranges_need_extent_review')

    def test_single_books_and_roman_ranges_are_distinct_from_volume_numbers(self):
        book = survey.parse_title('The History of Rome, Book V / The Military Monarchy')
        self.assertEqual((book['kind'], book['number']), ('book', 5))
        book = survey.parse_title('Roman History, Books I-III')
        self.assertEqual((book['scope'], book['range_end']), ('book_range', 3))
        groups = survey.group_candidates([self.entry('A Work, Book I'), self.entry('A Work, Volume II')])
        self.assertEqual(len(groups), 2)

    def test_descriptive_books_words_do_not_become_numbered_books(self):
        self.assertIsNone(survey.parse_title('The Books of the Bible'))
        self.assertIsNone(survey.parse_title('A Book for Free Spirits'))

    def test_book_ranges_from_different_authors_remain_separate(self):
        groups = survey.group_candidates([
            self.entry('The History of Rome, Books 01 to 08', creator='Livy'),
            self.entry('The History of Rome, Books 09 to 26', creator='Livy'),
            self.entry('The History of Rome, Book V', creator='Theodor Mommsen'),
        ])
        self.assertEqual(len(groups), 2)
        self.assertEqual(sorted(g['numbers'] for g in groups), [[1, 9], [5]])

    def test_abbreviated_v_requires_a_period_and_retains_extent(self):
        parsed = survey.parse_title('Philosophical Works, v. 2 (of 4)')
        self.assertEqual((parsed['kind'], parsed['number'], parsed['stated_total']), ('volume', 2, 4))
        self.assertIsNone(survey.parse_title('An Essay v 2'))

    def test_german_buch_is_a_book_division(self):
        parsed = survey.parse_title('Römische Geschichte — Buch 5')
        self.assertEqual((parsed['base_title'], parsed['kind'], parsed['number']), ('Römische Geschichte', 'book', 5))

    def test_parenthesized_positions_without_volume_word_are_discovered(self):
        groups = survey.group_candidates([self.entry('Los nueve libros de la Historia (1 de 2)', language='es'),
                                          self.entry('Los nueve libros de la Historia (2 de 2)', language='es')])
        self.assertEqual(groups[0]['numbers'], [1, 2])
        self.assertEqual(groups[0]['stated_totals'], [2])
        self.assertEqual(groups[0]['status'], 'contiguous_stated_range_needs_edition_review')
        self.assertIsNone(survey.parse_title('A History (9 of 2)'))

    def test_napoleon_alexander_slash_positions_form_a_complete_range(self):
        groups = survey.group_candidates([self.entry(f'Napoléon et Alexandre Ier ({n}/3) / Une histoire', language='fr')
                                          for n in range(1, 4)])
        self.assertEqual(len(groups), 1)
        self.assertEqual(groups[0]['title'], 'Napoléon et Alexandre Ier')
        self.assertEqual(groups[0]['numbers'], [1, 2, 3])
        self.assertEqual(groups[0]['status'], 'contiguous_stated_range_needs_edition_review')
        self.assertIsNone(survey.parse_title('Annual report (2025/2026)'))

    def test_dutch_deel_and_van_extent_are_discovered(self):
        groups = survey.group_candidates([self.entry(f'Geschiedenis van het Denken, Deel {n} van 2', language='nl')
                                          for n in range(1, 3)])
        self.assertEqual(groups[0]['title'], 'Geschiedenis van het Denken')
        self.assertEqual(groups[0]['numbers'], [1, 2])
        self.assertEqual(groups[0]['stated_totals'], [2])
        self.assertEqual(groups[0]['status'], 'contiguous_stated_range_needs_edition_review')
        self.assertEqual(survey.parse_title('Een werk, Deel tweede')['number'], 2)


if __name__ == '__main__':
    unittest.main()
