"""The private toolchain must run after its original package locations disappear."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch
import sys

spec = importlib.util.spec_from_file_location('bootstrap', Path(__file__).parents[1] / 'src/engines/adapters/nxnative/bootstrap_nxnative.py')
bootstrap = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bootstrap)

class RelocationTests(unittest.TestCase):
    def test_user_pip_overrides_cannot_redirect_private_installation(self):
        with patch.dict(os.environ, {'PIP_TARGET':'/outside/private/tools', 'PIP_USER':'1', 'PYTHONUSERBASE':'/outside/user/base'}):
            result = bootstrap.run([sys.executable, '-c', 'import os,json; print(json.dumps({k:v for k,v in os.environ.items() if k.startswith("PIP_") or k=="PYTHONUSERBASE"}))'])
        child = json.loads(result)
        self.assertNotIn('PIP_TARGET', child)
        self.assertNotIn('PIP_USER', child)
        self.assertNotIn('PYTHONUSERBASE', child)
        self.assertEqual(child['PIP_CONFIG_FILE'], os.devnull)

    def test_tools_use_their_private_libraries_after_relocation(self):
        with tempfile.TemporaryDirectory(prefix='rl tools with spaces ') as name:
            root = Path(name)
            original = root / 'original/libfixture.dylib'
            original.parent.mkdir()
            (root / 'library.c').write_text('int answer(void) { return 42; }')
            (root / 'main.c').write_text('#include <stdio.h>\nint answer(void); int main(void) { printf("%d", answer()); return 0; }')
            subprocess.run(['/usr/bin/clang','-dynamiclib',str(root/'library.c'),'-o',str(original),'-install_name',str(original)],check=True)
            tools = root / 'private tools'
            (tools/'bin').mkdir(parents=True)
            (tools/'lib').mkdir()
            subprocess.run(['/usr/bin/clang',str(root/'main.c'),str(original),'-o',str(tools/'bin/fixture')],check=True)
            (tools/'lib/libfixture.dylib').write_bytes(original.read_bytes())
            bootstrap.relocate(tools)
            original.unlink()
            result = subprocess.run([str(tools/'bin/fixture')],check=True,capture_output=True,text=True,env={'PATH':'/usr/bin:/bin'})
            self.assertEqual(result.stdout,'42')

    def test_missing_private_library_is_rejected(self):
        with tempfile.TemporaryDirectory(prefix='rl missing tool ') as name:
            root=Path(name)
            (root/'bin').mkdir()
            (root/'lib').mkdir()
            (root/'library.c').write_text('int answer(void) { return 42; }')
            (root/'main.c').write_text('int answer(void); int main(void) { return answer(); }')
            library=root/'outside.dylib'
            subprocess.run(['/usr/bin/clang','-dynamiclib',str(root/'library.c'),'-o',str(library),'-install_name',str(library)],check=True)
            subprocess.run(['/usr/bin/clang',str(root/'main.c'),str(library),'-o',str(root/'bin/fixture')],check=True)
            with self.assertRaisesRegex(RuntimeError,'missing private library'):
                bootstrap.relocate(root)

if __name__ == '__main__':
    unittest.main()
