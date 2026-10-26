"""Profile verification must require fresh frames from every selected model."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    'test_kobo_profiles_runner', Path(__file__).resolve().parents[1] / 'test-kobo-profiles.py')
RUNNER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RUNNER)

CATALOGUE = '310\ttrilogy\t6x8\tKobo Touch B\n388\tio\t6x8\tKobo Libra 2\n'
FRAME = b'P5\n6 8\n255\n' + bytes(range(48))


class ProfileRunnerTest(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.binary = self.root / 'application'
        self.binary.write_bytes(b'test executable')
        self.output = self.root / 'captures'
        self.output.mkdir()
        self.argv = ['test-kobo-profiles.py', '--binary', str(self.binary),
                     '--output', str(self.output)]

    def run_runner(self, catalogue=CATALOGUE, render=True, extra=()):
        def capture(command, **kwargs):
            if render:
                Path(command[command.index('--capture') + 1]).write_bytes(FRAME)

        with patch('sys.argv', [*self.argv, *extra]), patch.object(RUNNER, 'ROOT', self.root), \
                patch.object(RUNNER.subprocess, 'check_output', return_value=catalogue), \
                patch.object(RUNNER.subprocess, 'run', side_effect=capture) as run, \
                contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            RUNNER.main()
        return run.call_args_list

    def test_qemu_runs_each_model_with_isolated_data_and_reports_binary(self):
        calls = self.run_runner(extra=['--qemu', str(self.root / 'qemu-arm'),
                                       '--qemu-root', str(self.root / 'sysroot')])
        report = json.loads((self.output / 'report.json').read_text())
        self.assertEqual([profile['id'] for profile in report['profiles']], [310, 388])
        self.assertEqual(report['binary'], str(self.binary))
        self.assertIn('host kernel', report['kind'])
        self.assertEqual(len(calls), 2)
        for call, identifier in zip(calls, [310, 388]):
            command = call.args[0]
            self.assertEqual(command[:6], [str(self.root / 'qemu-arm'), '-cpu', 'cortex-a8',
                                           '-L', str(self.root / 'sysroot'), str(self.binary)])
            self.assertEqual(command[command.index('--device-model') + 1], str(identifier))
            self.assertEqual(call.kwargs['env']['BOKHEIM_KOBO_DATA_DIR'],
                             str(self.output / str(identifier) / 'data'))

    def test_missing_capture_cannot_reuse_an_old_success(self):
        previous = self.output / '310'
        previous.mkdir()
        (previous / 'application.pgm').write_bytes(FRAME)
        (self.output / 'report.json').write_text('{"profiles": [310]}')
        with self.assertRaises(FileNotFoundError):
            self.run_runner(render=False)
        self.assertFalse((self.output / 'report.json').exists())

    def test_empty_catalogue_cannot_pass(self):
        with self.assertRaisesRegex(RuntimeError, 'empty device catalogue'):
            self.run_runner(catalogue='')
        self.assertFalse((self.output / 'report.json').exists())

    def test_duplicate_profile_cannot_pass(self):
        with self.assertRaisesRegex(RuntimeError, 'duplicate device profile'):
            self.run_runner(catalogue=CATALOGUE + CATALOGUE.splitlines()[0] + '\n')

    def test_unknown_requested_model_fails_before_rendering(self):
        with self.assertRaises(SystemExit):
            self.run_runner(extra=['--models', '310', '999'])
        self.assertFalse((self.output / '310').exists())

    def test_model_filter_limits_captures(self):
        calls = self.run_runner(extra=['--models', '388'])
        self.assertEqual(len(calls), 1)
        report = json.loads((self.output / 'report.json').read_text())
        self.assertEqual([profile['id'] for profile in report['profiles']], [388])

    def test_same_aspect_ratio_at_wrong_resolution_fails(self):
        frame = self.root / 'wrong-resolution.pgm'
        frame.write_bytes(b'P5\n3 4\n255\n' + bytes(range(12)))
        with self.assertRaisesRegex(RuntimeError, 'wrong device dimensions'):
            RUNNER.inspect_frame(frame, 6, 8)

    def test_blank_or_truncated_capture_fails(self):
        frame = self.root / 'broken.pgm'
        for data, reason in [(b'\0' * 48, 'empty frame'), (bytes(range(47)), 'Incomplete frame')]:
            with self.subTest(reason=reason):
                frame.write_bytes(b'P5\n6 8\n255\n' + data)
                with self.assertRaisesRegex(RuntimeError, reason):
                    RUNNER.inspect_frame(frame, 6, 8)


if __name__ == '__main__':
    unittest.main()
