"""Archive build tools cannot alter the enclosing application's Git checkout."""
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    'prepare_kobo_runtime', Path(__file__).resolve().parents[1] / 'prepare-kobo-runtime.py')
RUNTIME = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RUNTIME)


class ArchiveIsolationTest(unittest.TestCase):
    def test_upstream_git_reset_cannot_reach_the_application_checkout(self):
        with tempfile.TemporaryDirectory() as temporary:
            repository = Path(temporary)
            subprocess.run(['git', 'init', '-q', str(repository)], check=True)
            sentinel = repository / 'local-edit.txt'
            sentinel.write_text('committed\n')
            subprocess.run(['git', '-C', str(repository), 'add', '.'], check=True)
            subprocess.run(['git', '-C', str(repository), '-c', 'user.name=Test',
                            '-c', 'user.email=test@example.invalid', 'commit', '-qm', 'fixture'], check=True)
            sentinel.write_text('uncommitted work\n')
            cache = repository / 'target/source-cache'
            archive = cache / 'unpacked-library/submodule'
            archive.mkdir(parents=True)
            with patch.object(RUNTIME, 'CACHE', cache):
                with self.assertRaises(subprocess.CalledProcessError):
                    RUNTIME.run(['git', 'reset', '--hard'], cwd=archive,
                                log=repository / 'build.log')
            self.assertEqual(sentinel.read_text(), 'uncommitted work\n')


if __name__ == '__main__':
    unittest.main()
