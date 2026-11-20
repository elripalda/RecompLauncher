#!/usr/bin/env python3
"""Import disc names, not compatibility claims, from the pinned runtime source."""
import argparse
import json
from pathlib import Path
import re


def collect(source, revision, artwork):
    titles = {}
    for path in sorted((source / 'Data/Sys/GameSettings').glob('*.ini')):
        header = path.read_text(encoding='utf-8', errors='replace').splitlines()
        if not header or not header[0].startswith('# ') or ' - ' not in header[0]:
            continue
        ids, name = header[0][2:].split(' - ', 1)
        for title in re.findall(r'\b[GRS][A-Z0-9]{5}\b', ids):
            record = {
                'title_id': title,
                'name': name.strip(),
                'console': 'gc' if title.startswith('G') else 'wii',
                'source': f'https://github.com/ExpansionPak/RecompCore/blob/{revision}/{path.relative_to(source).as_posix()}',
            }
            if title in artwork:
                record.update(artwork[title])
            titles.setdefault(title, record)
    return [titles[title] for title in sorted(titles)]


def normalize(name):
    return re.sub(r'[^a-z0-9]+', '', name.lower())


def reported_names(readme):
    section = readme.split('## Hall of Fame', 1)[-1].split('\n## ', 1)[0]
    names = []
    for line in section.splitlines():
        if not line.strip() or line.startswith('Contact '):
            continue
        if ' - ' in line:
            line = line.split(' - ', 1)[1]
        elif ')' in line:
            line = line.split(')', 1)[1]
        else:
            continue
        names.extend(re.sub(r'\s+recomp$', '', name.strip()) for name in line.split('&'))
    return names


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('--revision', required=True)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--artwork', type=Path)
    parser.add_argument('--readme', required=True, type=Path)
    parser.add_argument('--selection', type=Path, help='Optional JSON list of exact upstream disc IDs')
    args = parser.parse_args()
    if not re.fullmatch(r'[0-9a-f]{40}', args.revision):
        parser.error('revision must be an exact upstream commit')
    artwork = json.loads(args.artwork.read_text()) if args.artwork else {}
    candidates = collect(args.source, args.revision, artwork)
    names = reported_names(args.readme.read_text())
    matched = {normalize(name) for name in names}
    titles = [t for t in candidates if normalize(t['name']) in matched]
    for name in names:
        if not any(normalize(t['name']) == normalize(name) for t in candidates):
            print(f'Unmatched upstream name: {name}')
    if args.selection:
        selection = set(json.loads(args.selection.read_text()))
        titles = [t for t in candidates if t['title_id'] in selection]
        missing = selection - {t['title_id'] for t in titles}
        if missing:
            parser.error('selected IDs missing upstream metadata: ' + ', '.join(sorted(missing)))
    if args.artwork:
        titles = [t for t in titles if t['title_id'] in artwork]
    if not titles:
        parser.error('no exact disc metadata found in the supplied source')
    args.output.write_text(json.dumps(titles, ensure_ascii=False, indent=2) + '\n')
    print(f'Imported {len(titles)} unverified disc metadata records')


if __name__ == '__main__':
    main()
