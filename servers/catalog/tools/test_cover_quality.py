import unittest

from cover_quality import assessment, preferred_design, rank_key


def cover(ratings, size=(333, 500), **fields):
    return {'size': size, 'eligible': True,
            'design_review': assessment(ratings, 'Reviewed complete cover.'), **fields}


class CoverQualityTests(unittest.TestCase):
    def test_hiawatha_layout_beats_portrait_without_user_override(self):
        typography = cover((3, 3, 4, 2), (324, 500))
        portrait = cover((2, 2, 3, 3), (400, 500), has_illustration=True)
        self.assertIs(preferred_design([portrait, typography]), typography)

    def test_picture_presence_is_not_a_bonus(self):
        plain = cover((3, 3, 3, 3))
        picture = cover((3, 3, 3, 3), has_illustration=True, preferred_artwork=True)
        self.assertEqual(rank_key(plain), rank_key(picture))

    def test_artwork_context_guess_is_not_a_ranking_factor(self):
        neutral = cover((3, 3, 3, 2))
        guessed_context = cover((3, 3, 3, 4))
        self.assertEqual(rank_key(neutral), rank_key(guessed_context))

    def test_fitting_artwork_wins_when_complete_design_is_stronger(self):
        framed_text = cover((3, 3, 4, 2))
        fair_barbarian = cover((4, 4, 4, 4))
        self.assertIs(preferred_design([framed_text, fair_barbarian]), fair_barbarian)

    def test_small_title_cannot_be_compensated_by_good_artwork(self):
        readable = cover((3, 3, 4, 3))
        small_title = cover((4, 4, 2, 4))
        self.assertIs(preferred_design([small_title, readable]), readable)

    def test_ratio_distinguishes_comparable_designs(self):
        wide = cover((3, 3, 3, 3), (400, 500))
        portrait = cover((3, 3, 3, 3))
        self.assertIs(preferred_design([wide, portrait]), portrait)

    def test_user_preference_cannot_promote_wrong_work(self):
        correct = cover((3, 3, 3, 3))
        wrong = cover((4, 4, 4, 4), eligible=False, user_preferred=True)
        self.assertIs(preferred_design([wrong, correct]), correct)

    def test_fallback_and_unreviewed_are_excluded(self):
        self.assertIsNone(preferred_design([
            cover((2, 2, 3, 2)), {'eligible': True, 'size': (333, 500)}]))

    def test_full_work_preferred_to_single_volume_for_merged_book(self):
        full = cover((3, 3, 3, 3))
        volume = cover((4, 4, 4, 4), single_volume=True)
        self.assertIs(preferred_design([volume, full], merged=True), full)


if __name__ == '__main__':
    unittest.main()
