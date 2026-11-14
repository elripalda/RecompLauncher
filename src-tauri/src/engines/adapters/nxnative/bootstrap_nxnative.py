"""Assemble verified upstream tools privately; never install into the user's Python or Homebrew."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys


def run(args):
    env = {k:v for k,v in os.environ.items() if not k.startswith('PIP_') and k != 'PYTHONUSERBASE'}
    env['PIP_CONFIG_FILE'] = os.devnull
    return subprocess.run([str(a) for a in args], check=True, capture_output=True, text=True, env=env).stdout


def relocate(root):
    libraries = root / 'lib'
    for folder in [libraries, root / 'bin']:
        for path in sorted(folder.iterdir()):
            if path.is_symlink() or not path.is_file():
                continue
            with path.open('rb') as f:
                magic = f.read(4)
            if magic not in [b'\xcf\xfa\xed\xfe', b'\xfe\xed\xfa\xcf', b'\xca\xfe\xba\xbe', b'\xbe\xba\xfe\xca']:
                continue
            changes = []
            links = run(['/usr/bin/otool', '-L', path]).splitlines()[1:]
            for line in links:
                link = line.strip().split(' (', 1)[0]
                if link.startswith(('/usr/lib/', '/System/Library/')):
                    continue
                name = Path(link).name
                if not (libraries / name).is_file():
                    raise RuntimeError(f'{path.name}: missing private library {name}')
                relative = '@loader_path/' + ('' if folder == libraries else '../lib/') + name
                if relative != link:
                    changes.extend(['-change', link, relative])
            if folder == libraries:
                changes.extend(['-id', '@loader_path/' + path.name])
            if changes:
                run(['/usr/bin/install_name_tool', *changes, path])
            run(['/usr/bin/codesign', '--force', '--sign', '-', path])


def assemble(config):
    root = Path(config['destination'])
    shutil.copytree(config['python'], root / 'python', symlinks=True)
    for folder in ['bin', 'lib']:
        (root / folder).mkdir()
    for package in config['bottles']:
        package = Path(package)
        for folder in ['bin', 'lib']:
            source = package / folder
            if not source.is_dir():
                continue
            for path in source.iterdir():
                wanted = path.name in ['glslang', 'glslangValidator', 'zstd'] if folder == 'bin' else path.name.endswith('.dylib')
                if wanted:
                    shutil.copy2(path, root / folder / path.name, follow_symlinks=False)
    relocate(root)
    python = root / 'python/bin/python3'
    # Every wheel, including transitive dependencies, was hash-checked by the launcher.
    # No package index, resolver or user's pip configuration can change this set.
    run([python, '-m', 'pip', '--disable-pip-version-check', '--no-cache-dir', 'install', '--no-index', '--no-deps', '--no-compile', *config['wheels']])
    run([python, '-c', 'import capstone, lz4.block, fontTools, zbic, compression.zstd, nsz, numpy, PIL, texture2ddecoder'])
    run([root / 'bin/glslangValidator', '--version'])
    run([root / 'bin/zstd', '--version'])
    print(json.dumps({'ev':'stage_end', 'ok':True, 'name':'Components ready'}), flush=True)


if __name__ == '__main__':
    try:
        assemble(json.loads(Path(sys.argv[1]).read_text()))
    except Exception as error:
        print(json.dumps({'ev':'say', 'msg':'[RED] Could not prepare private build tools: ' + str(error)}), flush=True)
        raise
