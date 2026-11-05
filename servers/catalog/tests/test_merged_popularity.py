"""Raw component evidence is retained independently of snapshot percentiles."""
import importlib.util
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location('refresh', Path(__file__).parents[1] / 'tools/refresh-merged-popularity.py')
REFRESH = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(REFRESH)


def observation(identity, count):
    return {'downloads': count, 'scope': 'ebook', 'listing_url': f'https://www.gutenberg.org/ebooks/{identity}',
            'observed_at': '2026-10-30T00:00:00Z', 'download_metadata': 'downloads in the last 30 days', 'page_sha256': 'a' * 64}


class MergedPopularity(unittest.TestCase):
    def setUp(self):
        self.metadata = {'kind': 'combined_volumes', 'components': [
            {'record': {'description': 'Preserve this'}, 'source_entries': [
                {'source': 'gutenberg.org', 'entry_id': '1', 'metadata': {'downloads': 0, 'observed_at': 'earlier', 'cover': {'source': 'keep'}}}]},
            {'record': {'title': 'Volume 2'}, 'source_entries': [
                {'source': 'gutenberg.org', 'entry_id': '2', 'metadata': {}}]}]}
        self.observations = {'1': observation('1', 0), '2': observation('2', 3)}

    def test_fractional_mean_includes_zero_and_preserves_original_evidence(self):
        updated, mean, count = REFRESH.refresh(self.metadata, self.observations)
        self.assertEqual((mean, count), (1.5, 2))
        component = updated['components'][0]
        self.assertEqual(component['record'], self.metadata['components'][0]['record'])
        evidence = component['source_entries'][0]['metadata']
        self.assertEqual(evidence['cover'], {'source': 'keep'})
        self.assertIn({'downloads': 0, 'observed_at': 'earlier'}, evidence['download_observations'])
        self.assertIn(self.observations['1'], evidence['download_observations'])
        self.assertNotIn('download_observations', self.metadata['components'][0]['source_entries'][0]['metadata'])
        self.assertEqual(REFRESH.refresh(updated, self.observations)[0], updated)

    def test_missing_component_observation_cannot_make_a_partial_average(self):
        del self.observations['2']
        with self.assertRaises(KeyError):
            REFRESH.refresh(self.metadata, self.observations)

    def test_invalid_counts_and_wrong_page_are_rejected(self):
        for value in (True, -1, 1.5, '3', 2**64):
            self.observations['2']['downloads'] = value
            with self.assertRaises(ValueError):
                REFRESH.refresh(self.metadata, self.observations)
        self.observations['2'] = observation('99', 3)
        with self.assertRaises(ValueError):
            REFRESH.refresh(self.metadata, self.observations)


if __name__ == '__main__':
    unittest.main()
