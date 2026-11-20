import json
from pathlib import Path
import subprocess
import tempfile
import unittest

IMPORTER = Path(__file__).with_name('import_catalog.py')

class CatalogImport(unittest.TestCase):
    def test_disc_metadata_is_imported_without_game_specific_rules(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            metadata = root / 'Data/Sys/GameSettings'
            metadata.mkdir(parents=True)
            (metadata / 'GAB.ini').write_text('# GABE01, GABP01 - A Synthetic Game\n[Core]\n')
            (metadata / 'SXYE01.ini').write_text('# SXYE01 - Another Synthetic Game\n')
            (metadata / 'GNOE01.ini').write_text('# GNOE01 - Not mentioned upstream\n')
            (root / 'README.md').write_text('## Hall of Fame\nAuthor - A Synthetic Game & Another Synthetic Game recomp\nContact me for inclusion\n')
            (metadata / 'HA8.ini').write_text('# HA8xxx - A channel is not a disc\n')
            output = root / 'titles.json'
            result = subprocess.run(['python3', str(IMPORTER), str(root), '--revision', 'a' * 40, '--output', str(output), '--readme', str(root / 'README.md')], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            titles = json.loads(output.read_text())
            self.assertEqual([t['title_id'] for t in titles], ['GABE01', 'GABP01', 'SXYE01'])
            self.assertEqual(titles[0]['console'], 'gc')
            self.assertEqual(titles[2]['console'], 'wii')
            self.assertIn('/' + 'a' * 40 + '/Data/Sys/GameSettings/GAB.ini', titles[0]['source'])
            self.assertNotIn('version', titles[0])

if __name__ == '__main__':
    unittest.main()
