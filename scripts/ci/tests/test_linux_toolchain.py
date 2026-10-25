import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[3]
LINUX = ROOT / 'scripts/linux'


def module(name):
    spec = importlib.util.spec_from_file_location(name, LINUX / f'{name}.py')
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


class CompilerTests(unittest.TestCase):
    def test_build_scripts_can_load_native_sdk_sonames_on_the_build_host(self):
        compiler = module('compiler')
        arguments = ['main.o', '-o', '/target/build/package/build_script_build-123']
        result = compiler.compiler_arguments(arguments, Path('/sdk'), True)
        self.assertIn('-Wl,-rpath,/sdk/lib', result)
        arguments[-1] = '/target/release/bokheim'
        self.assertNotIn('-Wl,-rpath,/sdk/lib', compiler.compiler_arguments(arguments, Path('/sdk'), True))

    def test_host_targets_and_library_paths_cannot_override_the_sdk(self):
        compiler = module('compiler')
        sdk = Path('/sdk with spaces')
        arguments = ['--target=x86_64-unknown-linux-gnu', '-target', 'host',
                     '-L/usr/lib', '-L', '/usr/lib/x86_64-linux-gnu',
                     '-L', '/project/native', '-lstdc++', '-lgcc_s', 'app.o']
        self.assertEqual(compiler.compiler_arguments(arguments, sdk, True),
                         ['-L', '/project/native', str(sdk / 'lib/libstdc++.so.6'),
                          str(sdk / 'lib/libgcc_s.so.1'), 'app.o'])

    def test_corrupt_cached_download_is_rejected_even_offline(self):
        toolchain = module('toolchain')
        with tempfile.TemporaryDirectory() as directory:
            cache = Path(directory)
            (cache / 'archive').write_bytes(b'corrupt')
            with self.assertRaisesRegex(RuntimeError, 'checksum mismatch'):
                toolchain.download({'name': 'archive', 'sha256': '0' * 64}, cache, True)


@unittest.skipUnless(sys.platform == 'linux' and shutil.which('cc'), 'Linux C compiler required')
class LauncherTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='linux launcher ')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.app = self.root / 'AppDir'
        self.binary = self.app / 'usr/bin/probe'
        self.binary.parent.mkdir(parents=True)
        self.fallback = self.app / 'usr/lib/bokheim-compat'
        self.fallback.mkdir(parents=True)
        self.original = self.root / 'system-libraries'
        self.original.mkdir()
        self.env = {**os.environ, 'LD_LIBRARY_PATH': str(self.original)}
        self.env.pop('BOKHEIM_SYSTEM_LD_LIBRARY_PATH', None)

    def launch(self, *arguments):
        return subprocess.run(['sh', str(LINUX / 'launch.sh'), str(self.app), 'probe', *arguments],
                              env=self.env, text=True, capture_output=True, timeout=10)

    def test_system_libraries_are_preferred_and_original_path_is_saved(self):
        self.binary.symlink_to('/bin/sh')
        result = self.launch('-c', 'printf "%s\\n%s\\n%s" "$LD_LIBRARY_PATH" "$BOKHEIM_SYSTEM_LD_LIBRARY_PATH" "$1"',
                             'probe', 'argument with spaces')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.splitlines(), [str(self.original),
                         str(self.original), 'argument with spaces'])

    def test_missing_symbol_version_selects_fallback(self):
        source = self.root / 'runtime.c'
        source.write_text('int bokheim_runtime(void) { return 42; }\n')
        version = self.root / 'version.map'
        for number, destination in [('1', self.original), ('2', self.fallback)]:
            version.write_text(f'BOKHEIM_{number} {{ global: bokheim_runtime; }};\n')
            subprocess.run(['cc', '-shared', '-fPIC', str(source),
                            '-Wl,--version-script=' + str(version),
                            '-Wl,-soname,libbokheim-launch-test.so',
                            '-o', str(destination / 'libbokheim-launch-test.so')], check=True, capture_output=True)
        main = self.root / 'main.c'
        main.write_text('#include <stdio.h>\n#include <stdlib.h>\n'
                        'extern int bokheim_runtime(void);\n'
                        'int main(void) { puts(getenv("LD_LIBRARY_PATH")); return bokheim_runtime() != 42; }\n')
        subprocess.run(['cc', str(main), '-L' + str(self.fallback), '-lbokheim-launch-test',
                        '-o', str(self.binary)], check=True, capture_output=True)
        result = self.launch()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), f'{self.fallback}:{self.original}')


if __name__ == '__main__':
    unittest.main()
