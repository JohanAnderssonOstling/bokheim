"""Run the Wi-Fi deployment through a local fake SSH transport."""
import io
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest

SCRIPTS = Path(__file__).resolve().parents[1]


class KoboWifiDeploymentTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.device = self.root / 'device'
        (self.device / '.kobo').mkdir(parents=True)
        (self.device / '.adds').mkdir()
        self.firmware = self.root / 'firmware'
        (self.firmware / 'imageformats').mkdir(parents=True)
        (self.firmware / 'imageformats/libnm.so').write_bytes(b'NickelMenu installed')
        self.proc = self.root / 'proc'
        self.proc.mkdir()
        self.fake_bin = self.root / 'bin'
        self.fake_bin.mkdir()
        self.log = self.root / 'ssh-log'
        ssh = self.fake_bin / 'ssh'
        ssh.write_text('''#!/usr/bin/python3
import os, subprocess, sys
from pathlib import Path
command = sys.argv[-1]
with open(os.environ['KOBO_TEST_SSH_LOG'], 'a') as log:
    log.write(command + '\\n')
command = command.replace('/mnt/onboard', os.environ['KOBO_TEST_DEVICE'])
command = command.replace('/usr/local/Kobo', os.environ['KOBO_TEST_FIRMWARE'])
command = command.replace('/proc/', os.environ['KOBO_TEST_PROC'] + '/')
data = sys.stdin.buffer.read()
if 'cat >' in command and 'update.tar.gz' in command:
    if os.environ.get('KOBO_TEST_INTERRUPT_UPLOAD'):
        sys.exit(1)
    if os.environ.get('KOBO_TEST_CORRUPT_UPLOAD'):
        data = b'corrupted upload'
result = subprocess.run(['sh', '-c', command], input=data, capture_output=True)
sys.stdout.buffer.write(result.stdout.replace(os.environ['KOBO_TEST_DEVICE'].encode(), b'/mnt/onboard'))
sys.stderr.buffer.write(result.stderr)
sys.exit(result.returncode)
''')
        ssh.chmod(0o755)
        self.archive = self.root / 'package with spaces.tar.gz'
        with tarfile.open(self.archive, 'w:gz') as archive:
            for path, contents in {
                '.adds/bokheim/run.sh': b'launcher',
                '.adds/nm/desktop-gpui-kobo': b'old diagnostic menu',
            }.items():
                member = tarfile.TarInfo(path)
                member.size = len(contents)
                archive.addfile(member, io.BytesIO(contents))

    def run_deploy(self, **changes):
        environment = dict(os.environ, PATH=f'{self.fake_bin}:/usr/bin:/bin',
                           KOBO_TEST_SSH_LOG=str(self.log), KOBO_TEST_DEVICE=str(self.device),
                           KOBO_TEST_FIRMWARE=str(self.firmware), KOBO_TEST_PROC=str(self.proc),
                           BOKHEIM_KOBO_SSH_PORT='2222', **changes)
        return subprocess.run(['sh', str(SCRIPTS / 'deploy-kobo-wifi.sh'), 'root@kobo.test', str(self.archive)],
                              env=environment, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=10)

    def test_streams_and_verifies_package_and_installs_only_bokheim_menu(self):
        result = self.run_deploy()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.device / '.adds/bokheim/run.sh').read_bytes(), b'launcher')
        self.assertEqual((self.device / '.adds/nm/desktop-gpui-kobo').read_bytes(), (SCRIPTS.parent / 'kobo/nickelmenu').read_bytes())
        for installed, source in [('bokheim.png', 'bokheim-kobo.png'), ('bokheim-active.png', 'bokheim-kobo-active.png')]:
            self.assertEqual((self.device / '.adds/bokheim/nickelmenu-icons' / installed).read_bytes(),
                             (SCRIPTS.parent.parent / 'ui/design-tokens/assets/icon' / source).read_bytes())
        self.assertFalse(list((self.device / '.adds').glob('bokheim-update.*')))
        self.assertIn('Installed and verified Bokheim over Wi-Fi', result.stdout)

    def test_a_running_application_prevents_overwriting_its_executable(self):
        (self.proc / '123').mkdir()
        (self.proc / '123/comm').write_text('desktop-gpui-ko\n')
        result = self.run_deploy()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Quit Bokheim', result.stderr)
        self.assertFalse((self.device / '.adds/bokheim').exists())
        self.assertNotIn('cat >', self.log.read_text())

    def test_a_corrupted_upload_is_rejected_before_extraction(self):
        result = self.run_deploy(KOBO_TEST_CORRUPT_UPLOAD='1')
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.device / '.adds/bokheim').exists())
        self.assertFalse(list((self.device / '.adds').glob('bokheim-update.*')))

    def test_an_interrupted_upload_preserves_the_existing_application(self):
        (self.device / '.adds/bokheim').mkdir()
        original = self.device / '.adds/bokheim/run.sh'
        original.write_bytes(b'previous launcher')
        result = self.run_deploy(KOBO_TEST_INTERRUPT_UPLOAD='1')
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(original.read_bytes(), b'previous launcher')
        self.assertFalse(list((self.device / '.adds').glob('bokheim-update.*')))


if __name__ == '__main__':
    unittest.main()
