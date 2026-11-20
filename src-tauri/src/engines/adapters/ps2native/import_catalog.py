#!/usr/bin/env python3
"""Offline PS2 metadata import from pinned upstream snapshots; no game-specific mappings.

Arguments: snapshot directory containing the pinned source folder, GameIndex.yaml,
boxart-tree.json and snap-tree.json. Output is catalog-candidate.json and skipped-titles.json.
Compatibility text comes only from ps2-native-recomp. PCSX2 supplies names/serials, never
compatibility settings. Ambiguous identity matches are reported instead of chosen.
"""
from pathlib import Path
import json
import hashlib
import re
import sys
import unicodedata
from urllib.parse import quote

ROOT = Path(sys.argv[1]) if __name__ == '__main__' and len(sys.argv) > 1 else Path(__file__).parent
ENGINE_REVISION = '9b0eef16ecd5df327eb23eb5adaf844251c2f29f'
IDENTITY_REVISION = '15afef683235e4941ff7de15c2c19699811a1448'
ART_REVISION = 'c2fcaf5f39aa63285008f154fa64daa5f6815187'
COVER_REVISION = '08a8734ee31aca74a7471425f73384f16e29ecbc'


def normalized(name):
    name = unicodedata.normalize('NFKD', name).casefold().replace('&', ' and ')
    name = re.sub(r'\([^)]*\)|\[[^]]*\]', ' ', name)
    return re.sub(r'[^a-z0-9]', '', re.sub(r'\bthe\b', '', name))


def plain_name(name):
    return re.sub(r'\s*\((?:USA|Europe|Japan)\)$', '', name)


def names_match(reported, candidate):
    a, b = normalized(reported), normalized(candidate)
    if a == b:
        return 2
    # Full unbroken reported title with a database prefix/subtitle is conservative;
    # fuzzy spellings and translations require independent evidence and stay unresolved.
    if len(a) >= 8 and len(re.findall(r'\w+', plain_name(reported))) >= 2 and a in b:
        return 1
    return 0


def identities():
    text = (ROOT / 'GameIndex.yaml').read_text()
    records = []
    for block in re.split(r'\n(?=[A-Z][A-Z0-9]{3}-[0-9A-Za-z]+:)', text):
        serial = re.match(r'([A-Z]{4}-[0-9]{5}):', block)
        if not serial:
            continue
        def field(key):
            match = re.search(r'^  ' + key + r': "((?:[^"\\]|\\.)*)"', block, re.M)
            return match[1] if match else ''
        name, english = field('name'), field('name-en')
        if not name or re.search(r'\[(?:[^]]*Demo|Trial|Prototype)|Public Beta|Online Public Beta', name + english, re.I):
            continue
        records.append({'serial': serial[1], 'name': name, 'name_en': english, 'region': field('region')})
    return records


def requested_region(name):
    if name.endswith('(Japan)'):
        return 'Japan'
    if name.endswith('(Europe)'):
        return 'Europe'
    if name.endswith('(USA)'):
        return 'USA'
    return None


def region_matches(record, region):
    if region == 'Europe':
        return record['region'].startswith('PAL')
    return record['region'] == {'USA': 'NTSC-U', 'Japan': 'NTSC-J'}[region]


def choose_identity(title, records):
    scored = [(max(names_match(title['name'], r['name']), names_match(title['name'], r['name_en'])), r) for r in records]
    scored = [(score, record) for score, record in scored if score]
    if not scored:
        return [], None, 'No exact normalized title or full title phrase in primary metadata.'
    candidates = [record for _, record in scored]
    region = requested_region(title['name'])
    if not region:
        # USA is a catalog metadata edition choice, not an assertion about the report.
        # If there is no US edition, only a unique known regional group can be selected.
        if any(region_matches(record, 'USA') for record in candidates):
            region = 'USA'
        else:
            available = [r for r in ['Japan', 'Europe'] if any(region_matches(record, r) for record in candidates)]
            if len(available) != 1:
                return candidates, None, 'Report does not specify a region and metadata has no unique regional choice.'
            region = available[0]
    scored = [(score, record) for score, record in scored if region_matches(record, region)]
    best = max((score for score, _ in scored), default=0)
    candidates = [record for score, record in scored if score == best]
    if len(candidates) != 1:
        return candidates, region, 'No unique serial for the selected region; edition needs more evidence.'
    return candidates, region, None


def artwork(title, identity, region, paths):
    possible = []
    for path in paths:
        if not path.endswith('.png') or '(' + region not in path:
            continue
        if re.search(r'\([^)]*(?:Demo|Beta|Prototype|Translat|Hack|Modded|Patched)', path, re.I):
            continue
        stem = path[:-4]
        exact = max(names_match(title['name'], stem), names_match(identity['name'], stem), names_match(identity['name_en'], stem))
        if exact:
            possible.append((exact, -len(path), path))
    return max(possible)[2] if possible else None


def upstream_titles():
    readme = ROOT / ('ps2-native-recomp-' + ENGINE_REVISION) / 'README.md'
    rows = []
    in_section = False
    in_table = False
    for line_number, line in enumerate(readme.read_text().splitlines(), 1):
        if line.strip() == '## What runs':
            in_section = True
            continue
        if not in_section:
            continue
        if line.startswith('| Game | How far |'):
            in_table = True
            continue
        if not in_table:
            continue
        if line.startswith('|---'):
            continue
        if not line.startswith('|'):
            break
        columns = [value.strip() for value in line.strip('|').split('|')]
        if len(columns) != 3:
            raise SystemExit('Upstream title table changed shape; review the importer.')
        rows.append({'name': columns[0], 'notes': columns[1], 'tested_by': columns[2],
                     'evidence': f'https://github.com/exoteric33/ps2-native-recomp/blob/{ENGINE_REVISION}/README.md#L{line_number}'})
    if not rows:
        raise SystemExit('No supported-title table found at the pinned source.')
    return rows


def main():
    sources = json.loads(Path(__file__).with_name('catalog-sources.json').read_text())
    for name, source in sources.items():
        if hashlib.sha256((ROOT / name).read_bytes()).hexdigest() != source['sha256']:
            raise SystemExit('Metadata snapshot failed verification: ' + name)
    titles = upstream_titles()
    records = identities()
    snaps = json.loads((ROOT / 'snap-tree.json').read_text())
    boxes = json.loads((ROOT / 'boxart-tree.json').read_text())
    if snaps.get('truncated') or boxes.get('truncated'):
        raise SystemExit('Artwork index is truncated; refuse incomplete input.')
    paths = {entry['path'] for entry in snaps['tree'] if entry['type'] == 'blob'}
    catalog, skipped = [], []
    for title in titles:
        candidates, region, problem = choose_identity(title, records)
        if problem:
            skipped.append({'name': title['name'], 'reason': problem, 'candidates': candidates, 'evidence': title['evidence']})
            continue
        record = candidates[0]
        screenshot = artwork(title, record, region, paths)
        serial = record['serial']
        cover = f'https://raw.githubusercontent.com/xlenore/ps2-covers/{COVER_REVISION}/covers/default/{serial}.jpg'
        hero = f'https://raw.githubusercontent.com/libretro-thumbnails/Sony_-_PlayStation_2/{ART_REVISION}/Named_Snaps/{quote(screenshot)}' if screenshot else None
        catalog.append({
            'engine': 'ps2-native-recomp', 'title_id': serial, 'name': plain_name(title['name']),
            'console': 'ps2', 'package': '', 'version': '', 'build_id': '',
            'notes': title['notes'] + ' Upstream title-level report; this exact disc revision and Mac gameplay have not been verified by RecompLauncher.',
            'evidence': title['evidence'], 'tested_by': title['tested_by'],
            'identity_name': record['name'], 'identity_name_en': record['name_en'], 'region': region,
            'report_region_explicit': requested_region(title['name']) is not None,
            'identity_source': f'https://github.com/PCSX2/pcsx2/blob/{IDENTITY_REVISION}/bin/resources/GameIndex.yaml',
            'identity_revision': IDENTITY_REVISION, 'upstream_revision': ENGINE_REVISION,
            'cover': cover, 'hero': hero or cover, 'hero_kind': 'screenshot' if hero else 'cover', 'artwork_filename': screenshot,
            'artwork_source': f'https://github.com/libretro-thumbnails/Sony_-_PlayStation_2/tree/{ART_REVISION}/Named_Snaps',
            'cover_source': f'https://github.com/xlenore/ps2-covers/blob/{COVER_REVISION}/covers/default/{serial}.jpg',
        })
    (ROOT / 'catalog-candidate.json').write_text(json.dumps(catalog, ensure_ascii=False, indent=2) + '\n')
    (ROOT / 'skipped-titles.json').write_text(json.dumps(skipped, ensure_ascii=False, indent=2) + '\n')
    print(f'{len(catalog)} resolved title/serial records; {len(skipped)} unresolved titles; {sum(x["hero_kind"] != "screenshot" for x in catalog)} without screenshot.')
    for item in skipped:
        print(item['name'] + ': ' + item['reason'])
    for item in catalog:
        if item['hero_kind'] != 'screenshot':
            print('COVER USED AS HERO: ' + item['name'])


if __name__ == '__main__':
    main()
