import copy
import importlib.util
import json
import unittest
from pathlib import Path

ROOT = Path(__file__).parent
spec = importlib.util.spec_from_file_location('import_titles', ROOT / 'import_titles.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

class ImportTitles(unittest.TestCase):
    def setUp(self):
        self.reports = json.loads((ROOT / 'reports.json').read_text())
        self.checkpoint = json.loads((ROOT / 'report_checkpoint.json').read_text())

    def test_snapshot_is_reproducible(self):
        expected = json.loads((ROOT / 'titles.json').read_text())
        self.assertEqual(module.convert(self.reports, self.checkpoint), expected)

    def test_new_title_is_data_only(self):
        report = copy.deepcopy(self.reports[0])
        report['number'] = 20000
        report['url'] = 'https://github.com/boykopovar/AnyPS5/discussions/20000'
        report['body'] = report['body'].replace('PPSA07053', 'PPSA99999').replace('Coffee Talk Episode 2: Hibiscus & Butterfly', 'Another title')
        game = module.convert([report], self.checkpoint)[0]
        self.assertEqual(game['name'], 'Another title')
        self.assertEqual(game['title_id'], 'PPSA99999')
        self.assertEqual(game['version'], '')
        self.assertEqual(game['report']['reported_version'], '01.120.000')

    def test_forks_and_local_modifications_are_not_compatibility_evidence(self):
        report = copy.deepcopy(self.reports[0])
        report['body'] = report['body'].replace('with no pull requests or local changes on top.', 'with local changes and PR #42 on top.')
        self.assertEqual(module.convert([report], self.checkpoint), [])
        report['url'] = 'https://github.com/example/fork/discussions/20000'
        self.assertEqual(module.convert([report], self.checkpoint), [])

    def test_unknown_revision_is_not_attributed_to_the_release(self):
        report = copy.deepcopy(self.reports[0])
        report['body'] = report['body'].replace('b11c7bf84f', '1234567890')
        self.assertEqual(module.convert([report], self.checkpoint), [])

if __name__ == '__main__':
    unittest.main()
