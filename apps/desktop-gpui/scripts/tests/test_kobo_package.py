"""Reject packages that declare broad support but carry an incompatible libc."""
import importlib.util
from pathlib import Path
import struct
import unittest

SPEC = importlib.util.spec_from_file_location('verify_kobo_package', Path(__file__).resolve().parents[1] / 'verify-kobo-package.py')
VERIFY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VERIFY)


class KernelAbiTest(unittest.TestCase):
    def test_reads_the_arm_runtime_kernel_requirement(self):
        for version in [(2, 6, 32), (3, 2, 0), (3, 10, 0)]:
            elf = bytearray(128)
            elf[:6] = b'\x7fELF\x01\x01'
            struct.pack_into('<I', elf, 28, 52)
            struct.pack_into('<HH', elf, 42, 32, 1)
            struct.pack_into('<II', elf, 52, 4, 84)
            struct.pack_into('<I', elf, 68, 32)
            struct.pack_into('<III', elf, 84, 4, 16, 1)
            elf[96:100] = b'GNU\0'
            struct.pack_into('<IIII', elf, 100, 0, *version)
            self.assertEqual(VERIFY.minimum_kernel(elf), version)

    def test_missing_or_wrong_architecture_notes_are_rejected(self):
        with self.assertRaises(RuntimeError):
            VERIFY.minimum_kernel(b'not an ELF')
        elf = bytearray(52)
        elf[:6] = b'\x7fELF\x01\x01'
        with self.assertRaises(RuntimeError):
            VERIFY.minimum_kernel(elf)


if __name__ == '__main__':
    unittest.main()
