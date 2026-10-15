"""Verify SSH bootstrap permissions and failure handling without firmware writes."""
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import unittest


class KoboSshBootstrapTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.ssh = self.root / 'ssh'
        self.ssh.mkdir()
        self.state = self.root / 'internal'
        self.pid = self.root / 'server.pid'
        self.bin = self.root / 'bin'
        self.bin.mkdir()
        source = Path(__file__).resolve().parents[2] / 'kobo/ssh/start.sh'
        script = source.read_text().replace('/etc/bokheim-ssh', str(self.state))
        script = script.replace('/var/run/bokheim-ssh.pid', str(self.pid))
        (self.ssh / 'start.sh').write_text(script)
        (self.ssh / 'authorized_keys').write_text('ssh-ed25519 test-public-key\n')
        (self.ssh / 'hostkey-seed').write_bytes(b'unique-device-host-key')
        self.command('id', '#!/bin/sh\nprintf "0\\n"\n')
        self.command('readlink', f'#!/bin/sh\nprintf "%s\\n" "{self.ssh}/dropbear"\n')
        (self.ssh / 'dropbear').write_text('''#!/usr/bin/python3
import json, os, sys, time
from pathlib import Path
args = sys.argv[1:]
Path(__file__).with_name('arguments.json').write_text(json.dumps(args))
Path(args[args.index('-P') + 1]).write_text(str(os.getpid()))
time.sleep(20)
''')
        (self.ssh / 'dropbearkey').write_text('#!/bin/sh\nexit 1\n')
        self.addCleanup(self.stop_server)

    def command(self, name, text):
        path = self.bin / name
        path.write_text(text)
        path.chmod(0o755)

    def stop_server(self):
        if self.pid.is_file():
            try:
                os.kill(int(self.pid.read_text()), signal.SIGTERM)
            except ProcessLookupError:
                pass

    def start(self):
        return subprocess.run(['sh', str(self.ssh / 'start.sh')],
                              env=dict(os.environ, PATH=f'{self.bin}:/usr/bin:/bin'),
                              capture_output=True, text=True, timeout=5)

    def test_copies_authentication_to_private_internal_storage_and_starts_once(self):
        result = self.start()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.state.stat().st_mode & 0o777, 0o700)
        for name in ('hostkey', 'authorized_keys'):
            self.assertEqual((self.state / name).stat().st_mode & 0o777, 0o600)
        self.assertEqual((self.state / 'hostkey').read_bytes(), b'unique-device-host-key')
        self.assertFalse((self.ssh / 'hostkey-seed').exists())
        initial_pid = self.pid.read_text()
        self.assertEqual(self.start().returncode, 0)
        self.assertEqual(self.pid.read_text(), initial_pid)
        arguments = (self.ssh / 'arguments.json').read_text()
        self.assertIn('2222', arguments)
        self.assertIn(str(self.state), arguments)

    def test_missing_public_key_does_not_start_server(self):
        (self.ssh / 'authorized_keys').unlink()
        result = self.start()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('No Kobo SSH public key', result.stderr)
        self.assertFalse(self.pid.exists())

    def test_refuses_symlinked_internal_state(self):
        other = self.root / 'other'
        other.mkdir()
        self.state.symlink_to(other, target_is_directory=True)
        self.assertNotEqual(self.start().returncode, 0)
        self.assertFalse(list(other.iterdir()))

    def test_refuses_symlinked_key_staging_file(self):
        self.state.mkdir()
        protected = self.root / 'protected'
        protected.write_bytes(b'preserve')
        (self.state / 'authorized_keys.new').symlink_to(protected)
        self.assertNotEqual(self.start().returncode, 0)
        self.assertEqual(protected.read_bytes(), b'preserve')
        self.assertFalse(self.pid.exists())


if __name__ == '__main__':
    unittest.main()
