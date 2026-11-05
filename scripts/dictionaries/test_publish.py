import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import zipfile


module = importlib.util.spec_from_file_location("publish_dictionaries", Path(__file__).with_name("publish.py"))
publish = importlib.util.module_from_spec(module)
module.loader.exec_module(publish)


class Download(io.BytesIO):
    def __init__(self, data, length=None):
        super().__init__(data)
        self.headers = {} if length is None else {"Content-Length": str(length)}


class PublishTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.output = Path(self.temporary.name) / "dictionaries"
        archive = io.BytesIO()
        with zipfile.ZipFile(archive, "w") as writer:
            writer.writestr("de-de/LICENSE", "CC-BY-SA-4.0 license text")
            writer.writestr("de-de/ATTRIBUTION", "Wiktionary contributors")
            writer.writestr("de-de/dictionary.ifo", "Dictionary fixture")
        self.data = archive.getvalue()
        self.spec = dict(id="de-test-1", bytes=len(self.data), sha256=hashlib.sha256(self.data).hexdigest(),
                         upstream_url="https://example.org/de.zip", source_url="https://example.org", license="CC-BY-SA-4.0")
        self.target = self.output / f"{self.spec['sha256']}.zip"

    def test_publishes_exact_archive_with_notices_and_reuses_verified_file(self):
        with patch.object(publish, "open_archive", return_value=Download(self.data)) as request:
            self.assertTrue(publish.publish_dictionary(self.spec, self.output))
            self.assertFalse(publish.publish_dictionary(self.spec, self.output))
            request.assert_called_once_with(self.spec["upstream_url"])
        self.assertEqual(self.target.read_bytes(), self.data)
        self.assertEqual(self.target.stat().st_mode & 0o777, 0o644)
        self.assertEqual(list(self.output.iterdir()), [self.target])

    def test_corrupt_truncated_and_oversized_downloads_are_not_published(self):
        for data, length in [(b"x" * len(self.data), None), (self.data[:-1], None),
                             (self.data + b"x", None), (self.data, len(self.data) + 1)]:
            with self.subTest(length=length, bytes=len(data)):
                with patch.object(publish, "open_archive", return_value=Download(data, length)):
                    with self.assertRaises(ValueError):
                        publish.publish_dictionary(self.spec, self.output)
                self.assertEqual(list(self.output.iterdir()), [])

    def test_interrupted_download_cleans_up_and_retains_older_editions(self):
        self.output.mkdir()
        older = self.output / f"{'a' * 64}.zip"
        older.write_bytes(b"old edition")
        with patch.object(publish, "open_archive", side_effect=OSError("interrupted")):
            with self.assertRaises(OSError):
                publish.publish_dictionary(self.spec, self.output)
        self.assertEqual(list(self.output.iterdir()), [older])
        self.assertEqual(older.read_bytes(), b"old edition")

    def test_verify_only_never_downloads_or_creates_missing_directory(self):
        with patch.object(publish, "open_archive") as request:
            with self.assertRaisesRegex(ValueError, "Missing published archive"):
                publish.publish_dictionary(self.spec, self.output, verify_only=True)
            request.assert_not_called()
        self.assertFalse(self.output.exists())

    def test_existing_corruption_is_reported_without_fetching_or_overwriting(self):
        self.output.mkdir()
        self.target.write_bytes(b"corrupt")
        with patch.object(publish, "open_archive") as request:
            with self.assertRaisesRegex(ValueError, "does not match"):
                publish.publish_dictionary(self.spec, self.output)
            request.assert_not_called()
        self.assertEqual(self.target.read_bytes(), b"corrupt")

    def test_catalog_rejects_unsafe_filenames_duplicate_ids_and_unbounded_sizes(self):
        path = Path(self.temporary.name) / "catalog.json"
        for catalog in [[dict(self.spec, sha256="../../archive")], [self.spec, self.spec],
                        [dict(self.spec, bytes=publish.MAX_BYTES + 1)],
                        [dict(self.spec, upstream_url="http://example.org/de.zip")]]:
            with self.subTest(catalog=catalog):
                path.write_text(json.dumps(catalog))
                with self.assertRaises(ValueError):
                    publish.load_catalog(path)

    def test_redirects_cannot_downgrade_https_downloads(self):
        with self.assertRaisesRegex(ValueError, "stay on HTTPS"):
            publish.HttpsRedirects().redirect_request(None, None, 302, "Found", {}, "http://example.org/file.zip")

    def test_prepared_archive_is_pinned_and_verified_without_rebuilding(self):
        with patch.object(publish, "open_archive", return_value=Download(self.data)):
            publish.publish_dictionary(self.spec, self.output)

        def prepare(command, check):
            self.assertTrue(check)
            with zipfile.ZipFile(command[-1], "w") as archive:
                archive.writestr("dictionary.idx", b"Original StarDict index")
                archive.writestr("dictionary.dict.dz", b"Original dictzip definitions")
                archive.writestr("dictionary.ifo", b"Original StarDict metadata")
                archive.writestr("dictionary.license.txt", "CC-BY-SA-4.0 license text")

        with patch.object(publish.subprocess, "run", side_effect=prepare) as compiler:
            prepared = publish.prepare_dictionary(self.spec, self.output, Path("prepare"))
            spec = dict(self.spec, prepared=prepared)
            self.assertEqual(publish.prepare_dictionary(spec, self.output, None, verify_only=True), prepared)
            compiler.assert_called_once()
        target = self.output / f"{prepared['sha256']}.zip"
        self.assertEqual(target.stat().st_size, prepared["bytes"])
        self.assertEqual(target.stat().st_mode & 0o777, 0o644)
        with zipfile.ZipFile(target) as archive:
            self.assertEqual(archive.read("dictionary.license.txt"), b"CC-BY-SA-4.0 license text")

    def test_bad_preparation_is_not_published(self):
        self.output.mkdir()

        def missing_index(command, check):
            with zipfile.ZipFile(command[-1], "w") as archive:
                archive.writestr("dictionary.ifo", b"Missing index")

        with patch.object(publish.subprocess, "run", side_effect=missing_index):
            with self.assertRaisesRegex(ValueError, "needs original StarDict"):
                publish.prepare_dictionary(self.spec, self.output, Path("prepare"))
        self.assertEqual(list(self.output.iterdir()), [])
        with self.assertRaisesRegex(ValueError, "Missing prepared"):
            publish.prepare_dictionary(self.spec, self.output, None, verify_only=True)

    def test_incomplete_and_custom_index_formats_are_not_published(self):
        self.output.mkdir()
        for original_index in [False, True]:
            def prepare(command, check):
                with zipfile.ZipFile(command[-1], "w") as archive:
                    archive.writestr("dictionary.search", b"BOKDIDX\0\x03\0\0\0")
                    archive.writestr("dictionary.ifo", b"Original metadata")
                    archive.writestr("dictionary.dict", b"Original body")
                    if original_index:
                        archive.writestr("dictionary.idx", b"Original index")
            with patch.object(publish.subprocess, "run", side_effect=prepare):
                with self.assertRaises(ValueError):
                    publish.prepare_dictionary(self.spec, self.output, Path("prepare"))
            self.assertEqual(list(self.output.iterdir()), [])

    def test_cached_custom_format_is_rejected_even_when_its_hash_matches(self):
        self.output.mkdir()
        data = io.BytesIO()
        with zipfile.ZipFile(data, "w") as archive:
            archive.writestr("dictionary.search", b"BOKDIDX\0\x03\0\0\0")
            archive.writestr("dictionary.ifo", b"Original metadata")
            archive.writestr("dictionary.dict", b"Original body")
        contents = data.getvalue()
        prepared = dict(bytes=len(contents), sha256=hashlib.sha256(contents).hexdigest())
        (self.output / f"{prepared['sha256']}.zip").write_bytes(contents)
        with self.assertRaisesRegex(ValueError, "needs original StarDict"):
            publish.prepare_dictionary(dict(self.spec, prepared=prepared), self.output, None, verify_only=True)


if __name__ == "__main__":
    unittest.main()
