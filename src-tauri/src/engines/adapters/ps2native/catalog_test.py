import importlib.util
import sys
from pathlib import Path
import unittest

sys.dont_write_bytecode = True

SPEC = importlib.util.spec_from_file_location('catalog', Path(__file__).with_name('import_catalog.py'))
catalog = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(catalog)


class CatalogTests(unittest.TestCase):
    def test_normalizes_title_punctuation_without_a_title_mapping(self):
        self.assertEqual(catalog.normalized('The Sample: Game (USA)'), 'samplegame')

    def test_explicit_region_precedes_shorter_name_in_another_region(self):
        records = [{'serial':'SLUS-12345','name':'Series - Persona 4','name_en':'','region':'NTSC-U'},
                   {'serial':'SLPM-12345','name':'Persona 4','name_en':'','region':'NTSC-J'}]
        result, region, problem = catalog.choose_identity({'name':'Persona 4 (USA)'}, records)
        self.assertIsNone(problem)
        self.assertEqual(len(result), 1)
        self.assertEqual(result[0]['serial'], 'SLUS-12345')

    def test_ambiguous_editions_are_reported(self):
        records = [{'serial':'SLUS-12345','name':'Example','name_en':'','region':'NTSC-U'},
                   {'serial':'SLUS-12346','name':'Example','name_en':'','region':'NTSC-U'}]
        result, region, problem = catalog.choose_identity({'name':'Example (USA)'}, records)
        self.assertIsNotNone(problem)
        self.assertEqual(len(result), 2)

    def test_artwork_excludes_unofficial_translations(self):
        paths = ['Persona 4 ( Translated PtBr) (USA).png', 'Shin Megami Tensei - Persona 4 (USA).png']
        identity = {'name': 'Shin Megami Tensei - Persona 4', 'name_en': ''}
        selected = catalog.artwork({'name': 'Persona 4'}, identity, 'USA', paths)
        self.assertEqual(selected, paths[1])

    def test_unverified_translations_are_not_guessed(self):
        records = [{'serial':'SLPM-12345','name':'Other title','name_en':'Unrelated Romaji','region':'NTSC-J'}]
        result, region, problem = catalog.choose_identity({'name':'English Name (Japan)'}, records)
        self.assertEqual(result, [])
        self.assertIsNotNone(problem)


if __name__ == '__main__':
    unittest.main()
