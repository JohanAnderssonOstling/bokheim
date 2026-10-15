"""Exercise deployment with temporary volumes and mocked host eject commands."""
import io
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest

SCRIPTS = Path(__file__).resolve().parents[1]
NICKELMENU = SCRIPTS.parent / 'kobo/nickelmenu-install/KoboRoot.tgz'


class KoboDeploymentTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.device = self.root / 'Kobo with spaces'
        (self.device / '.kobo').mkdir(parents=True)
        (self.device / '.kobo/version').write_text('test,4.1.15,4.38.23828,test')
        self.archive = self.root / 'app.tar.gz'
        self.fake_bin = self.root / 'bin'
        self.fake_bin.mkdir()
        self.log = self.root / 'commands'
        for name, code in {
            'findmnt': 'printf "/dev/fakekobo\\n"',
            'sync': 'exit 0',
            'udisksctl': '''
[ -f "$KOBO_TEST_DEVICE/.adds/bokheim/run.sh" ] || exit 91
cmp "$KOBO_TEST_PAYLOAD" "$KOBO_TEST_DEVICE/.kobo/KoboRoot.tgz" || exit 92
if [ "$1" = unmount ]; then exit "${KOBO_TEST_UNMOUNT_STATUS:-0}"; fi
exit "${KOBO_TEST_POWER_STATUS:-0}"
''',
        }.items():
            command = self.fake_bin / name
            command.write_text('#!/bin/sh\nprintf "%s %s\\n" "' + name + '" "$*" >> "$KOBO_TEST_LOG"\n' + code + '\n')
            command.chmod(0o755)

    def run_deploy(self, files=None, **extra_env):
        files = files or {'.adds/bokheim/run.sh': b'launcher', '.adds/nm/desktop-gpui-kobo': b'menu'}
        with tarfile.open(self.archive, 'w:gz') as archive:
            for name, content in files.items():
                entry = tarfile.TarInfo(name)
                entry.size = len(content)
                archive.addfile(entry, io.BytesIO(content))
        env = dict(os.environ, PATH=f'{self.fake_bin}:/usr/bin:/bin',
                   BOKHEIM_KOBO_ARCHIVE=str(self.archive), KOBO_TEST_LOG=str(self.log),
                   KOBO_TEST_DEVICE=str(self.device), KOBO_TEST_PAYLOAD=str(NICKELMENU), **extra_env)
        return subprocess.run(['sh', str(SCRIPTS / 'deploy-kobo.sh'), str(self.device)],
                              env=env, text=True, capture_output=True, timeout=10)

    def test_copies_nickelmenu_and_verifies_install_before_eject(self):
        result = self.run_deploy()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.device / '.kobo/KoboRoot.tgz').read_bytes(), NICKELMENU.read_bytes())
        for installed, source in [('bokheim.png', 'bokheim-kobo.png'), ('bokheim-active.png', 'bokheim-kobo-active.png')]:
            self.assertEqual((self.device / '.adds/bokheim/nickelmenu-icons' / installed).read_bytes(),
                             (SCRIPTS.parent.parent / 'ui/design-tokens/assets/icon' / source).read_bytes())
        calls = self.log.read_text()
        self.assertLess(calls.index('unmount'), calls.index('power-off'))
        self.assertIn('Kobo safely ejected', result.stdout)

    def test_permissions_failure_keeps_install_and_requests_manual_eject(self):
        result = self.run_deploy(KOBO_TEST_UNMOUNT_STATUS='1')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('Installation succeeded, but automatic eject failed', result.stderr)
        self.assertNotIn('power-off', self.log.read_text())
        self.assertTrue((self.device / '.adds/bokheim/run.sh').exists())

    def test_power_off_failure_is_still_a_safe_unmount(self):
        result = self.run_deploy(KOBO_TEST_POWER_STATUS='1')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('Kobo safely unmounted', result.stdout)

    def test_pending_firmware_is_preserved_without_writes_or_eject(self):
        pending = self.device / '.kobo/KoboRoot.tgz'
        pending.write_bytes(b'unrelated firmware')
        result = self.run_deploy()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(pending.read_bytes(), b'unrelated firmware')
        self.assertFalse((self.device / '.adds').exists())
        self.assertFalse(self.log.exists())

    def test_unsafe_archive_fails_before_writes_or_eject(self):
        result = self.run_deploy(files={'.adds/bokheim/run.sh': b'launcher', '.kobo/KoboReader.sqlite': b'bad'})
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.device / '.adds').exists())
        self.assertFalse(self.log.exists())


if __name__ == '__main__':
    unittest.main()
