import unittest

from cover_quality import assessment, rank_key


def cover(ratings, size=(333, 500), **fields):
    return {'size': size, 'eligible': True,
            'design_review': assessment(ratings, 'Reviewed complete cover.'), **fields}


class CoverQualityTests(unittest.TestCase):
    def test_picture_presence_is_not_a_bonus(self):
        plain = cover((3, 3, 3, 3))
        picture = cover((3, 3, 3, 3), has_illustration=True, preferred_artwork=True)
        self.assertEqual(rank_key(plain), rank_key(picture))

    def test_artwork_context_guess_is_not_a_ranking_factor(self):
        neutral = cover((3, 3, 3, 2))
        guessed_context = cover((3, 3, 3, 4))
        self.assertEqual(rank_key(neutral), rank_key(guessed_context))

    def test_actual_cover_precedes_title_page_with_stronger_metadata(self):
        actual = cover((2, 2, 3), metadata_match='loose', image_kind='cover')
        title_page = cover((4, 4, 4), metadata_match='strict', image_kind='page_scan')
        self.assertLess(rank_key(actual), rank_key(title_page))

    def test_edition_language_does_not_change_design_order(self):
        known = cover((3, 3, 3), edition_language_unverified=False)
        unknown = cover((3, 3, 3), edition_language_unverified=True)
        self.assertEqual(rank_key(known), rank_key(unknown))


if __name__ == '__main__':
    unittest.main()
