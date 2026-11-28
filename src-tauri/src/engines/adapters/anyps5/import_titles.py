"""Import metadata from reviewed upstream discussion snapshots; never generate game code.

Input is a JSON list of GitHub GraphQL Discussion objects (reports.json). Commit
ancestry is reviewed separately in report_checkpoint.json. Output is a candidate
file, not an automatic replacement of the catalog or installed-game pins.
"""
import argparse
import hashlib
import json
import re
from pathlib import Path


def sections(body):
    pieces = re.split(r'^### (.+)\n', body, flags=re.MULTILINE)
    return {pieces[i].strip(): pieces[i + 1].strip() for i in range(1, len(pieces), 2)}


def candidate(report, checkpoint):
    prefix = f"https://github.com/{checkpoint['repository']}/discussions/"
    if report.get('category', {}).get('name') != 'Game bring-up':
        return None
    if report.get('url') != prefix + str(report.get('number')):
        return None
    if report.get('updatedAt', '') > checkpoint['report_cutoff']:
        return None
    body = report['body']
    fields = sections(body)
    revision = re.fullmatch(r'`main` ([0-9a-f]{7,40}) \(\d{4}-\d{2}-\d{2}\), with no pull requests or local changes on top\. The executable was converted by that commit\'s relinker on the day of the run\.', fields.get('AnyPS5 commit', ''))
    if not revision:
        return None
    ancestor = checkpoint['verified_ancestors'].get(revision[1])
    if not ancestor or ancestor['behind_by'] != 0 or not ancestor['revision'].startswith(revision[1]):
        return None
    if 'Environment: none; no trace or profiling switches.' not in fields.get('How it was run', ''):
        return None
    if not fields.get('System tested', '').startswith('Windows '):
        return None
    if fields.get('Furthest point') != 'In game':
        return None
    title = re.fullmatch(r'(.+) \((PPSA[0-9]{5})\)', fields.get('Title', ''))
    version = fields.get('Content version tested', '')
    if not title or not re.fullmatch(r'\d{2}\.\d{3}\.\d{3}', version):
        return None
    images = re.findall(r'src="(https://github.com/user-attachments/assets/[a-f0-9-]+)"', fields.get('What is shown', ''))
    if not images:
        return None
    return {
        'engine': 'anyps5', 'title_id': title[2], 'name': title[1], 'console': 'ps5',
        'package': '', 'version': '', 'build_id': '',
        'notes': f"Experimental generic conversion. Unverified with AnyPS5 {checkpoint['release']}; an upstream Windows report reached gameplay on an older source revision. This is title metadata, not a supported-build profile.",
        'evidence': report['url'], 'source_revision': checkpoint['release_revision'],
        'report': {
            'created_at': report['createdAt'], 'updated_at': report['updatedAt'],
            'source_revision': ancestor['revision'], 'reported_version': version,
            'body_sha256': hashlib.sha256(body.encode()).hexdigest(), 'platform': 'windows-x64',
            'stage': 'in-game', 'retrieved_at': checkpoint['retrieved_at'],
        },
        'screenshots': images[:2] if len(images) > 1 else images * 2,
    }


def convert(reports, checkpoint):
    result = []
    seen = set()
    for report in reports:
        game = candidate(report, checkpoint)
        if not game:
            continue
        if game['title_id'] in seen:
            raise ValueError('Review duplicate title reports before importing: ' + game['title_id'])
        seen.add(game['title_id'])
        result.append(game)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('reports', type=Path)
    parser.add_argument('checkpoint', type=Path)
    parser.add_argument('candidate_output', type=Path)
    args = parser.parse_args()
    records = convert(json.loads(args.reports.read_text()), json.loads(args.checkpoint.read_text()))
    with args.candidate_output.open('x', encoding='utf-8') as output:
        output.write(json.dumps(records, ensure_ascii=False, indent=2) + '\n')
    print(f'{len(records)} title metadata candidates; gameplay with the pinned release remains unverified.')


if __name__ == '__main__':
    main()
