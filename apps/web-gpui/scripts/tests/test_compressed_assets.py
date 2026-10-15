"""Exercise HTTP negotiation and decoded asset contents using the real server."""

from functools import partial
import gzip
import http.client
from http.server import ThreadingHTTPServer
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import threading
import unittest


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    loaded = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(loaded)
    return loaded


SCRIPTS = Path(__file__).resolve().parents[1]
compressor = module("compress_assets", SCRIPTS / "compress-assets.py")
server = module("serve", SCRIPTS / "serve.py")


class QuietHandler(server.CrossOriginIsolatedHandler):
    def log_message(self, *_):
        pass


class CompressedAssetsTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory = tempfile.TemporaryDirectory()
        root = Path(cls.directory.name)
        cls.payload = b"\x00asm\x01\x00\x00\x00"
        cls.source = root / "module.wasm"
        cls.source.write_bytes(cls.payload)
        compressor.compress_asset(cls.source)
        index = root / "index.html"
        index.write_bytes(b"<!doctype html><title>Compressed index</title>")
        compressor.compress_asset(index)
        (root / "plain.wasm").write_bytes(cls.payload)
        (root / "reference.sqlite3.gz").write_bytes(gzip.compress(b"SQLite reference", mtime=0))
        cls.server = ThreadingHTTPServer(("127.0.0.1", 0), partial(QuietHandler, directory=root))
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join()
        cls.directory.cleanup()

    def request(self, encoding, path="/module.wasm", method="GET"):
        connection = http.client.HTTPConnection(*self.server.server_address)
        connection.request(method, path, headers={"Accept-Encoding": encoding})
        response = connection.getresponse()
        status, headers, body = response.status, dict(response.getheaders()), response.read()
        connection.close()
        return status, headers, body

    def test_brotli_is_preferred_and_preserves_wasm_mime_and_contents(self):
        status, headers, body = self.request("gzip, br")
        self.assertEqual(status, 200)
        self.assertEqual(headers["Content-Encoding"], "br")
        self.assertEqual(headers["Content-Type"], "application/wasm")
        decoded = subprocess.run(["brotli", "-d", "-c"], input=body, capture_output=True, check=True).stdout
        self.assertEqual(decoded, self.payload)
        self.assertEqual(headers["Vary"], "Accept-Encoding")
        self.assertEqual(headers["Cross-Origin-Embedder-Policy"], "require-corp")

    def test_gzip_fallback(self):
        _, headers, body = self.request("gzip")
        self.assertEqual(headers["Content-Encoding"], "gzip")
        self.assertEqual(gzip.decompress(body), self.payload)

    def test_directory_index_uses_brotli(self):
        status, headers, body = self.request("br", "/")
        self.assertEqual(status, 200)
        self.assertEqual(headers["Content-Encoding"], "br")
        self.assertTrue(headers["Content-Type"].startswith("text/html"))
        decoded = subprocess.run(["brotli", "-d", "-c"], input=body, capture_output=True, check=True).stdout
        self.assertIn(b"Compressed index", decoded)

    def test_encoding_quality_and_explicit_rejection(self):
        for encoding in ("br;q=0, gzip", "br;q=0.5, gzip;q=1", "*;q=1, br;q=0"):
            with self.subTest(encoding=encoding):
                _, headers, body = self.request(encoding)
                self.assertEqual(headers["Content-Encoding"], "gzip")
                self.assertEqual(gzip.decompress(body), self.payload)

    def test_identity_and_uncompressed_host_fallback(self):
        for encoding, path in (("identity", "/module.wasm"), ("br;q=0, gzip;q=0", "/module.wasm"), ("br, gzip", "/plain.wasm")):
            with self.subTest(encoding=encoding, path=path):
                _, headers, body = self.request(encoding, path)
                self.assertNotIn("Content-Encoding", headers)
                self.assertEqual(body, self.payload)

    def test_head_describes_the_compressed_body(self):
        _, headers, body = self.request("br", method="HEAD")
        self.assertEqual(body, b"")
        self.assertEqual(int(headers["Content-Length"]), self.source.with_suffix(".wasm.br").stat().st_size)

    def test_already_compressed_reference_is_not_encoded_again(self):
        _, headers, body = self.request("br, gzip", "/reference.sqlite3.gz")
        self.assertNotIn("Content-Encoding", headers)
        self.assertEqual(gzip.decompress(body), b"SQLite reference")

    def test_sidecars_are_deterministic(self):
        previous = [self.source.with_suffix(".wasm" + suffix).read_bytes() for suffix in (".gz", ".br")]
        compressor.compress_asset(self.source)
        self.assertEqual(previous, [self.source.with_suffix(".wasm" + suffix).read_bytes() for suffix in (".gz", ".br")])

    def test_sidecars_retain_server_read_access(self):
        for suffix in (".br", ".gz"):
            self.assertEqual(self.source.stat().st_mode & 0o777, self.source.with_suffix(".wasm" + suffix).stat().st_mode & 0o777)


if __name__ == "__main__":
    unittest.main()
