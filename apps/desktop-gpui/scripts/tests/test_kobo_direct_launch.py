"""Check the direct-launch extension and the installer payload that ships it."""
import hashlib
from pathlib import Path
import re
import shutil
import struct
import subprocess
import tarfile
import tempfile
import unittest

SCRIPTS = Path(__file__).resolve().parents[1]
REPO = SCRIPTS.parents[2]


class KoboDirectLaunchTests(unittest.TestCase):
    @unittest.skipUnless(shutil.which('c++'), 'A host C++ compiler is required')
    def test_direct_action_and_menu_fallbacks(self):
        with tempfile.TemporaryDirectory() as directory:
            binary = Path(directory) / 'direct-launch-test'
            subprocess.run(['c++', '-std=c++11', '-Wall', '-Wextra', '-Werror',
                            str(SCRIPTS / 'tests/nickelmenu_direct_launch.cc'), '-o', str(binary)],
                           check=True, capture_output=True, timeout=30)
            subprocess.run([str(binary)], check=True, capture_output=True, timeout=5)

    def test_installer_ships_patched_arm_plugin_with_matching_checksums(self):
        payload = SCRIPTS.parent / 'kobo/nickelmenu-install/KoboRoot.tgz'
        digest = hashlib.sha256(payload.read_bytes()).hexdigest()
        for path, pattern in [
            (REPO / 'apps/kobo-installer/src/lib.rs', r'const NICKEL_MENU_SHA256: &str = "([a-f0-9]{64})"'),
            (SCRIPTS / 'install-nickelmenu.sh', r'SHA256=([a-f0-9]{64})'),
            (payload.parent / 'VERSION.txt', r'SHA256: ([a-f0-9]{64})'),
        ]:
            self.assertEqual(re.search(pattern, path.read_text()).group(1), digest, str(path))
        with tarfile.open(payload) as archive:
            plugin = archive.extractfile('./usr/local/Kobo/imageformats/libnm.so').read()
        self.assertEqual(plugin[:5], b'\x7fELF\x01')  # ELF32
        self.assertEqual(plugin[5], 1)  # Little endian
        self.assertEqual(struct.unpack_from('<H', plugin, 18)[0], 40)  # ARM
        self.assertIn(b'menu_main_15505_direct\0', plugin)
        self.assertIn(b'v0.6.0-bokheim.1\0', plugin)
        config = (SCRIPTS.parent / 'kobo/nickelmenu').read_text()
        label = re.search(r'experimental\s*:menu_main_15505_direct\s*:(.+)', config).group(1).strip()
        self.assertRegex(config, rf'menu_item\s*:main\s*:{re.escape(label)}\s*:cmd_spawn\s*:quiet:/bin/sh /mnt/onboard/\.adds/bokheim/run-monitored\.sh')


if __name__ == '__main__':
    unittest.main()
