"""Prepare one PS2 disc with the pinned upstream pipeline and private frozen tools."""
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import shlex
import struct
import subprocess
import sys


def run(args, env=None, cwd=None):
    print('Preparing: ' + Path(str(args[0])).name, flush=True)
    subprocess.run([str(arg) for arg in args], env=env, cwd=cwd, check=True)


def capture(args):
    return subprocess.check_output([str(arg) for arg in args], text=True).strip()


def digest(path):
    with Path(path).open('rb') as file:
        return hashlib.file_digest(file, 'sha256').hexdigest()


def boot_program(config):
    match = re.search(r'^\s*BOOT2\s*=\s*(?:cdrom0?:)?[\\/]?([^\r\n;]+)(?:;1)?\s*$', config.lstrip('\ufeff'), re.I | re.M)
    if not match or not re.fullmatch(r'[A-Za-z]{4}_\d{3}\.\d{2}', match[1].strip()):
        raise ValueError('The PS2 disc has no safe serial-named BOOT2 executable.')
    return match[1].strip()


def identity(config, elf, selected):
    program = boot_program(config)
    serial = (program[:4] + '-' + program[5:8] + program[9:]).upper()
    if serial != selected.upper():
        raise ValueError('These files belong to a different game.')
    if len(elf) < 52 or elf[:7] != b'\x7fELF\x01\x01\x01' or struct.unpack_from('<H', elf, 18)[0] != 8:
        raise ValueError('The disc executable is not a PS2 MIPS ELF.')
    checksum = hashlib.sha256(elf).hexdigest()
    revision = re.search(r'^\s*VER\s*=\s*([^\r\n]+)', config, re.I | re.M)
    version = revision[1].strip() if revision else 'elf-' + checksum[:12]
    return {'console': 'ps2', 'title_id': serial, 'version': version, 'build_id': checksum}


def output_record(name, transformed):
    base = '{install}/Game/'
    args = [base + 'game/core.elf'] if transformed else []
    args += ['--disc', base + 'game/disc.iso', '--game-dir', base + 'cache', '--window',
             '--memcard', base + 'userdata/cards/card.ps2', '--quick-dir', base + 'userdata', '--no-record']
    environment = {'PS2_WINDOW_TITLE': name, 'PS2_VU_CXX': base + 'engine/compiler/bin/cxx',
                   'PS2_EE_LIVE': base + 'cache/ee-live', 'PS2_VU_LIVE': base + 'cache/vu1-live',
                   'PS2_KEYS': base + 'userdata/settings/keys.ini', 'PS2_VU_LIVE_OPT': '-O3',
                   'PS2_EE_LIVE_OPT': '-O1', 'PS2_EE_LIVE_QUICK': '-O0'}
    return {'start': {'program': ['engine/ps2run'], 'args': args, 'env': environment},
            'saves': ['userdata/cards', 'userdata/states', 'userdata/settings'],
            'files': ['engine', 'game', 'cache', 'result.json', 'userdata/reports', 'userdata/logs'], 'required_files': []}


def copy_tree(source, destination):
    # SDK/framework links stay internal; copied executable links are materialized below.
    shutil.copytree(source, destination, symlinks=True)


def copy_sdk(source, destination):
    # Manuals are not compiler inputs and contain names outside the portable path contract.
    share = Path(source) / 'usr/share'
    def ignore(folder, names):
        omitted = []
        for name in names:
            path = Path(folder) / name
            if Path(folder) == share and name in ('man', 'doc'):
                omitted.append(name)
                continue
            # Recursive header aliases add no files and would make a copied tree cyclic.
            if path.is_symlink() and path.is_dir() and Path(folder).resolve().is_relative_to(path.resolve()):
                omitted.append(name)
        return omitted
    shutil.copytree(source, destination, symlinks=True, ignore=ignore)


def compiler_wrapper(mode):
    return ('#!/bin/sh\nset -eu\n'
            'unset SDKROOT CPATH C_INCLUDE_PATH CPLUS_INCLUDE_PATH OBJC_INCLUDE_PATH LIBRARY_PATH COMPILER_PATH DYLD_LIBRARY_PATH DYLD_INSERT_LIBRARIES\n'
            'root=$(CDPATH= cd -- "$(/usr/bin/dirname -- "$0")/.." && pwd)\n'
            'exec "$root/bin/clang" --driver-mode=' + mode + ' -isysroot "$root/sdk" '
            '-resource-dir "$root/resources" -B"$root/bin" -fuse-ld="$root/bin/ld" "$@"\n')


def freeze_compiler(destination):
    clang = Path(capture(['/usr/bin/xcrun', '--find', 'clang++'])).resolve()
    linker = Path(capture(['/usr/bin/xcrun', '--find', 'ld'])).resolve()
    sdk = Path(capture(['/usr/bin/xcrun', '--show-sdk-path'])).resolve()
    resources = Path(capture([clang, '-print-resource-dir'])).resolve()
    destination.mkdir(parents=True)
    (destination / 'bin').mkdir()
    (destination / 'lib').mkdir()
    shutil.copy2(clang, destination / 'bin/clang')
    shutil.copy2(linker, destination / 'bin/ld')
    copy_tree(resources, destination / 'resources')
    copy_sdk(sdk, destination / 'sdk')
    todo = [destination / 'bin/clang', destination / 'bin/ld']
    while todo:
        program = todo.pop()
        changes = []
        for line in capture(['/usr/bin/otool', '-L', program]).splitlines()[1:]:
            dependency = line.strip().split(' (', 1)[0]
            if dependency.startswith(('/usr/lib/', '/System/Library/')):
                continue
            name = Path(dependency).name
            private = destination / 'lib' / name
            if not private.exists():
                source = clang.parent.parent / 'lib' / name
                if not source.is_file():
                    raise ValueError('Xcode compiler dependency is missing: ' + name)
                shutil.copy2(source, private)
                todo.append(private)
            relative = '@loader_path/' + ('../lib/' if program.parent.name == 'bin' else '') + name
            changes += ['-change', dependency, relative]
        if program.parent.name == 'lib':
            changes += ['-id', '@loader_path/' + program.name]
        if changes:
            run(['/usr/bin/install_name_tool', *changes, program])
            run(['/usr/bin/codesign', '--force', '--sign', '-', program])
    for name, mode in [('cc', 'gcc'), ('cxx', 'g++')]:
        wrapper = destination / 'bin' / name
        wrapper.write_text(compiler_wrapper(mode))
        wrapper.chmod(0o755)
    licenses = destination / 'licenses'
    licenses.mkdir()
    for parent in [clang.parent.parent.parent, sdk]:
        for source in parent.glob('*[Ll]icense*'):
            if source.is_file():
                shutil.copy2(source, licenses / source.name)
    for parent in clang.parents:
        license = parent / 'Contents/Resources/en.lproj/License.rtf'
        if parent.suffix == '.app' and license.is_file():
            shutil.copy2(license, licenses / 'Apple-Xcode-License.rtf')
    receipt = {'compiler_version': capture([clang, '--version']), 'sdk_version': capture(['/usr/bin/xcrun', '--show-sdk-version']),
               'files': {str(file.relative_to(destination)): digest(file) for file in sorted(destination.rglob('*')) if file.is_file()}}
    (destination / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    return destination / 'bin/cc', destination / 'bin/cxx'


def prepare(config):
    work, output = Path(config['work']), Path(config['output'])
    source = work / 'source'
    if source.exists():
        shutil.rmtree(source)
    copy_tree(config['source'], source)
    for name in ['game', 'cache', 'userdata/cards', 'userdata/states', 'userdata/settings', 'engine/licenses']:
        (output / name).mkdir(parents=True, exist_ok=True)
    disc = output / 'game/disc.iso'
    shutil.copy2(config['disc'], disc)
    extracted = work / 'identity'
    extracted.mkdir(exist_ok=True)
    run([sys.executable, '-I', source / 'scripts/disc.py', 'extract', disc, extracted, 'SYSTEM.CNF'])
    cnf = (extracted / 'SYSTEM.CNF').read_text(encoding='utf-8-sig')
    program = boot_program(cnf)
    run([sys.executable, '-I', source / 'scripts/disc.py', 'extract', disc, extracted, program])
    identified = identity(cnf, (extracted / program).read_bytes(), config['title_id'])
    cc, cxx = freeze_compiler(output / 'engine/compiler')
    cmake = Path(config['cmake']) / 'CMake.app/Contents/bin/cmake'
    ninja = Path(config['ninja']) / 'ninja'
    private = work / 'tools'
    private.mkdir(exist_ok=True)
    shell = private / 'bash'
    environment = {key: value for key, value in os.environ.items() if not key.startswith(('PS2_', 'CMAKE_', 'PYTHON', 'CXX', 'CPP', 'CCACHE', 'CFLAGS', 'LDFLAGS', 'LD_', 'DYLD_', 'CPATH', 'LIBRARY_PATH', 'COMPILER_PATH', 'SDKROOT', 'MACOSX_DEPLOYMENT_TARGET'))}
    environment.update(PATH=f'{private}:{cmake.parent}:{ninja.parent}:{Path(sys.executable).parent}:/usr/bin:/bin:/usr/sbin:/sbin',
                       CC=str(cc), CXX=str(cxx), PS2_AVX2='0', PS2_CCACHE='0', PS2_JOBS='4', PS2_SHARED_RUNTIME='0',
                       CMAKE_GENERATOR='Ninja', CMAKE_OSX_ARCHITECTURES='arm64')
    shell_ready = shell.is_file() and subprocess.run([shell, '-c', 'builtin compgen -A builtin compgen'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode == 0
    if not shell_ready:
        bash_source = work / 'bash-source'
        if bash_source.exists():
            shutil.rmtree(bash_source)
        copy_tree(config['bash'], bash_source)
        compiler_command = private / 'rl-ps2-cc'
        compiler_command.write_text('#!/bin/sh\nexec ' + shlex.quote(str(cc)) + ' "$@"\n')
        compiler_command.chmod(0o755)
        shell_environment = dict(environment, CC='rl-ps2-cc')
        run(['/bin/sh', './configure', '--disable-nls', '--without-bash-malloc'], shell_environment, bash_source)
        run(['/usr/bin/make', '-j4'], shell_environment, bash_source)
        shutil.copy2(bash_source / 'bash', shell)
    sdl_build, sdl_install = work / 'sdl-build', private / 'sdl'
    run([cmake, '-S', config['sdl'], '-B', sdl_build, '-G', 'Ninja', '-DCMAKE_BUILD_TYPE=Release',
         '-DCMAKE_C_COMPILER=' + str(cc), '-DCMAKE_INSTALL_PREFIX=' + str(sdl_install), '-DCMAKE_OSX_ARCHITECTURES=arm64',
         '-DSDL_SHARED=OFF', '-DSDL_STATIC=ON', '-DSDL_TEST=OFF', '-DSDL_TESTS=OFF'], environment)
    run([cmake, '--build', sdl_build, '--parallel', '4'], environment)
    run([cmake, '--install', sdl_build], environment)
    environment['SDL2_DIR'] = str(sdl_install / 'lib/cmake/SDL2')
    environment['CMAKE_PREFIX_PATH'] = str(sdl_install)
    copy_tree(config['wgpu'], source / 'third_party/wgpu-native')
    # Pre-stage a real file. Upstream otherwise symlinks player files outside the prepared tree.
    game = source / 'games/selected'
    game.mkdir(parents=True)
    os.link(disc, game / 'disc.iso')
    command = [shell, source / 'scripts/port.sh', 'selected', disc, '--native-only']
    if config.get('smoke'):
        command += ['--no-explore']
    run(command, environment, source)
    selected_compiler = (source / 'build/cxx_flags').read_text().splitlines()[1]
    if selected_compiler != str(cxx):
        raise ValueError('The upstream build selected a compiler outside the frozen package.')
    runner = source / 'build/native-selected/ps2run'
    if not runner.is_file() or 'arm64' not in capture(['/usr/bin/file', runner]):
        raise ValueError('The upstream build did not produce an arm64 native game.')
    # A successful headless build is insufficient: ensure the binary has its window implementation.
    if '_SDL_CreateWindow' not in capture(['/usr/bin/nm', runner]):
        raise ValueError('The prepared game is missing SDL window support.')
    shutil.copy2(runner, output / 'engine/ps2run')
    for line in capture(['/usr/bin/otool', '-L', runner]).splitlines()[1:]:
        dependency = line.strip().split(' (', 1)[0]
        if not dependency.startswith(('/usr/lib/', '/System/Library/')):
            raise ValueError('The game still depends on a nonportable library: ' + dependency)
    transformed = (game / 'core.elf').is_file()
    if transformed:
        shutil.copy2(game / 'core.elf', output / 'game/core.elf')
    for name in ['entries.txt', 'overlays', 'modules']:
        item = game / name
        if item.is_dir():
            copy_tree(item, output / 'cache' / name)
        elif item.is_file():
            shutil.copy2(item, output / 'cache' / name)
    for name in ['LICENSE', 'runtime/src/ipu/NOTICE']:
        shutil.copy2(source / name, output / 'engine/licenses' / Path(name).name)
    shutil.copy2(Path(config['sdl']) / 'LICENSE.txt', output / 'engine/licenses/SDL2.txt')
    for name, path in config['licenses'].items():
        shutil.copy2(path, output / 'engine/licenses' / name)
    for path in Path(config['wgpu']).rglob('*'):
        if path.is_file() and path.name.lower().startswith(('license', 'notice')):
            shutil.copy2(path, output / 'engine/licenses' / ('wgpu-' + path.name))
    result = output_record(config['name'], transformed)
    result['identity'] = identified
    result['required_files'] = [str(file.relative_to(output)) for file in sorted((output / 'engine').rglob('*')) if file.is_file()]
    result['required_files'] += ['game/disc.iso'] + (['game/core.elf'] if transformed else [])
    if any(any(char in path for char in ':{}\\') for path in result['required_files']):
        raise ValueError('The private runtime contains a path outside the portable installation contract.')
    (output / 'result.json').write_text(json.dumps(result, indent=2) + '\n')


if __name__ == '__main__':
    prepare(json.loads(Path(sys.argv[1]).read_text()))
