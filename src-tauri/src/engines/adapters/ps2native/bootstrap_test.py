import importlib.util
import sys
import os
import subprocess
from pathlib import Path
import struct
import tempfile
import unittest

sys.dont_write_bytecode = True

SPEC = importlib.util.spec_from_file_location('bootstrap', Path(__file__).with_name('bootstrap.py'))
bootstrap = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(bootstrap)


def elf():
    data = bytearray(64)
    data[:7] = b'\x7fELF\x01\x01\x01'
    struct.pack_into('<HH', data, 16, 2, 8)
    return bytes(data)


class IdentityTests(unittest.TestCase):
    def test_boot_serial_and_elf_determine_identity(self):
        value = bootstrap.identity('BOOT2 = cdrom0:\\SCUS_974.81;1\r\nVER = 1.01\r\n', elf(), 'SCUS-97481')
        self.assertEqual(value['title_id'], 'SCUS-97481')
        self.assertEqual(value['version'], '1.01')
        self.assertEqual(len(value['build_id']), 64)

    def test_wrong_disc_is_rejected_before_compiling(self):
        with self.assertRaisesRegex(ValueError, 'different game'):
            bootstrap.identity('BOOT2=cdrom0:\\SLUS_200.62;1\nVER=1.00', elf(), 'SCUS-97481')

    def test_wrong_elf_architecture_is_rejected(self):
        data = bytearray(elf())
        struct.pack_into('<H', data, 18, 62)
        with self.assertRaisesRegex(ValueError, 'PS2'):
            bootstrap.identity('BOOT2=cdrom0:\\SCUS_974.81;1\nVER=1.00', data, 'SCUS-97481')

    def test_missing_revision_is_identified_without_inventing_version(self):
        value = bootstrap.identity('BOOT2=cdrom0:\\SCUS_974.81;1', elf(), 'SCUS-97481')
        self.assertTrue(value['version'].startswith('elf-'))

    def test_escaping_boot_path_is_rejected(self):
        with self.assertRaises(ValueError):
            bootstrap.boot_program('BOOT2=cdrom0:\\..\\SCUS_974.81;1')

    def test_runtime_profile_keeps_caches_out_of_saves(self):
        value = bootstrap.output_record('Example title', False)
        self.assertEqual(value['start']['program'], ['engine/ps2run'])
        self.assertIn('--disc', value['start']['args'])
        self.assertEqual(value['start']['env']['PS2_VU_CXX'], '{install}/Game/engine/compiler/bin/cxx')
        self.assertNotIn('cache', value['saves'])
        self.assertIn('userdata/cards', value['saves'])
        self.assertNotIn('core.elf', ' '.join(value['start']['args']))

    def test_sdk_copy_keeps_build_headers_and_omits_unused_manuals(self):
        with tempfile.TemporaryDirectory(prefix='SDK copy ') as temporary:
            source, destination = Path(temporary) / 'sdk', Path(temporary) / 'frozen'
            (source / 'usr/include').mkdir(parents=True)
            (source / 'usr/share/man/man3').mkdir(parents=True)
            (source / 'usr/share/doc').mkdir()
            (source / 'usr/include/stdio.h').write_text('header')
            (source / 'usr/share/man/man3/APR::Base64.3pm').write_text('manual')
            (source / 'usr/share/doc/unused.txt').write_text('documentation')
            bootstrap.copy_sdk(source, destination)
            self.assertEqual((destination / 'usr/include/stdio.h').read_text(), 'header')
            self.assertFalse((destination / 'usr/share/man').exists())
            self.assertFalse((destination / 'usr/share/doc').exists())

    def test_sdk_copy_omits_recursive_directory_aliases(self):
        with tempfile.TemporaryDirectory(prefix='SDK link copy ') as temporary:
            source, destination = Path(temporary) / 'sdk', Path(temporary) / 'frozen'
            (source / 'usr/include').mkdir(parents=True)
            (source / 'usr/include/api.h').write_text('header')
            (source / 'usr/include/again').symlink_to('.', target_is_directory=True)
            (source / 'headers').symlink_to('usr/include', target_is_directory=True)
            bootstrap.copy_sdk(source, destination)
            self.assertFalse((destination / 'usr/include/again').exists())
            self.assertEqual((destination / 'headers/api.h').read_text(), 'header')

    def test_frozen_compiler_wrapper_moves_and_ignores_host_search_paths(self):
        with tempfile.TemporaryDirectory(prefix='private compiler ü ') as temporary:
            root = Path(temporary)
            (root / 'bin').mkdir()
            driver = root / 'bin/clang'
            driver.write_text('#!/bin/sh\nprintf "%s\\n" "${SDKROOT-unset}" "${CPATH-unset}" "$@"\n')
            driver.chmod(0o755)
            wrapper = root / 'bin/cxx'
            wrapper.write_text(bootstrap.compiler_wrapper('g++'))
            wrapper.chmod(0o755)
            environment = dict(os.environ, SDKROOT='/host/sdk', CPATH='/host/headers', PATH='/nonexistent')
            args = subprocess.check_output([wrapper, 'source with spaces.cpp'], env=environment, text=True).splitlines()
            self.assertEqual(args[:2], ['unset', 'unset'])
            self.assertIn(str(root / 'sdk'), args)
            self.assertIn(str(root / 'resources'), args)
            self.assertEqual(args[-1], 'source with spaces.cpp')

    def test_transformed_upstream_executable_is_used_when_present(self):
        value = bootstrap.output_record('Example title', True)
        self.assertEqual(value['start']['args'][0], '{install}/Game/game/core.elf')


if __name__ == '__main__':
    unittest.main()
