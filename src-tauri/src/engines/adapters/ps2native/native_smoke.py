#!/usr/bin/env python3
"""Build a synthetic disc with verified component roots, then test a movable native package.

Run with the pinned standalone Python:
  python3 native_smoke.py --components /path/to/extracted-components.json --work /tmp/new-ps2-smoke
The JSON maps source, cmake, ninja, sdl, bash and wgpu to checksum-verified extraction roots; licenses maps license names to verified files.
This opt-in test uses no game data, network, window or GPU. The output remains for inspection.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys

sys.dont_write_bytecode = True

SPEC = importlib.util.spec_from_file_location('bootstrap', Path(__file__).with_name('bootstrap.py'))
bootstrap = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(bootstrap)


def synthetic_elf():
    # This loop writes executable instructions into an empty code window. With
    # exploration disabled, only the runtime compiler can execute that window.
    code = [0x3c080020, 0x3c091000, 0x3529ffff, 0xad090000, 0x24090000, 0xad090004, 0x01000008, 0]
    data = bytearray(0x100 + 4 * len(code))
    data[:7] = b'\x7fELF\x01\x01\x01'
    struct.pack_into('<HHIIIIIHHHHHH', data, 16, 2, 8, 1, 0x100000, 52, 0, 0, 52, 32, 2, 40, 0, 0)
    struct.pack_into('<8I', data, 52, 1, 0x100, 0x100000, 0, 4 * len(code), 4 * len(code), 5, 4)
    struct.pack_into('<8I', data, 84, 1, 0, 0x200000, 0, 0, 0x1000, 7, 4)
    struct.pack_into('<' + 'I' * len(code), data, 0x100, *code)
    return data


def run_game(package, log, cold):
    environment = {'PATH': '/nonexistent', 'HOME': str(package / 'userdata'), 'PS2_GS': 'cpu',
                   'PS2_VU_CXX': str(package / 'engine/compiler/bin/cxx'), 'PS2_EE_LIVE': str(package / 'cache/ee-live'),
                   'PS2_VU_LIVE': '0', 'PS2_EE_LIVE_BG': '0', 'PS2_EE_LIVE_QUICK': 'off',
                   'SDKROOT': '/nonexistent/host-sdk', 'CPATH': '/nonexistent/host-headers'}
    args = [package / 'engine/ps2run', '--disc', package / 'game/disc.iso', '--game-dir', package / 'cache',
            '--memcard', package / 'userdata/cards/card.ps2', '--quick-dir', package / 'userdata', '--frames', '30', '--no-record']
    result = subprocess.run([str(arg) for arg in args], env=environment, cwd=package.parent, text=True,
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=180)
    log.write_text(result.stdout)
    assert result.returncode == 0 and 'stopped after 30 frames' in result.stdout, result.stdout[-8000:]
    if cold:
        assert 'switched in after' in result.stdout, result.stdout
    else:
        assert 'loaded 1 translated code windows' in result.stdout and 'switched in after' not in result.stdout, result.stdout


def exercise(package, source):
    root = package.parent
    for path in package.rglob('*'):
        if path.is_symlink():
            assert path.exists() and path.resolve().is_relative_to(package), f'Nonportable SDK link: {path}'
    record = json.loads((package / 'result.json').read_text())
    assert all((package / path).is_file() for path in record['required_files'])
    assert (package / 'engine/compiler/licenses/Apple-Xcode-License.rtf').is_file()
    assert (package / 'engine/licenses/wgpu-LICENSE-MIT.txt').is_file()
    assert (package / 'engine/licenses/wgpu-LICENSE-APACHE.txt').is_file()
    source.rename(source.with_name('source-unavailable'))
    run_game(package, root / 'cold.log', True)
    run_game(package, root / 'warm.log', False)
    moved = root / 'moved package ü'
    package.rename(moved)
    run_game(moved, root / 'moved-warm.log', False)
    shutil.rmtree(moved / 'cache/ee-live')
    run_game(moved, root / 'moved-cold.log', True)
    run_game(moved, root / 'moved-repeated.log', False)
    print('Native package: cold compilation, warm cache, moved warm/cold/repeated runs passed with no host tool search path.')


def exercise_registered(installation, profile_path):
    profile = json.loads(profile_path.read_text())
    start = profile['start']
    package = installation / 'Game'
    cache = package / 'cache/ee-live'
    if cache.exists():
        shutil.rmtree(cache)
    environment = {'PATH': '/nonexistent', 'HOME': str(package / 'userdata')}
    environment.update({key: value.replace('{install}', str(installation)) for key, value in start['env'].items()})
    environment.update(PS2_GS='cpu', PS2_VU_LIVE='0', PS2_EE_LIVE_BG='0', PS2_EE_LIVE_QUICK='off',
                       SDKROOT='/nonexistent/sdk', CPATH='/nonexistent/includes')
    args = [str(installation / start['program'][0])]
    args += [arg.replace('{install}', str(installation)) for arg in start['args'] if arg != '--window']
    args += ['--frames', '30']
    for cold, name in [(True, 'registered-cold.log'), (False, 'registered-warm.log')]:
        result = subprocess.run(args, cwd=installation, env=environment, text=True, stdout=subprocess.PIPE,
                                stderr=subprocess.STDOUT, timeout=180)
        (installation / name).write_text(result.stdout)
        assert result.returncode == 0 and 'stopped after 30 frames' in result.stdout, result.stdout[-8000:]
        if cold:
            assert 'switched in after' in result.stdout, result.stdout
        else:
            assert 'loaded 1 translated code windows' in result.stdout and 'switched in after' not in result.stdout, result.stdout
    print('Registered saved profile: cold/warm native execution passed.')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exercise-registered', type=Path)
    parser.add_argument('--profile', type=Path)
    parser.add_argument('--components', type=Path)
    parser.add_argument('--work', type=Path)
    args = parser.parse_args()
    if args.exercise_registered:
        exercise_registered(args.exercise_registered, args.profile)
        return
    if not args.work or not args.components:
        parser.error('--components and --work are required when building a fixture')
    work = args.work.resolve()
    work.mkdir(parents=True, exist_ok=False)
    components = json.loads(args.components.read_text())
    elf = work / 'SCUS_974.81'
    elf.write_bytes(synthetic_elf())
    disc = work / 'synthetic.iso'
    subprocess.run([sys.executable, '-I', str(Path(components['source']) / 'scripts/mkiso.py'), str(disc), str(elf)], check=True)
    output = work / 'output'
    output.mkdir()
    config = dict(components, disc=str(disc), work=str(work), output=str(output), title_id='SCUS-97481', name='Synthetic PS2 fixture', smoke=True)
    bootstrap.prepare(config)
    elf.unlink()
    exercise(output, work / 'source')


if __name__ == '__main__':
    main()
