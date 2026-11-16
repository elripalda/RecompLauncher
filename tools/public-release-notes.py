#!/usr/bin/env python3
"""Render player-facing release notes from the maintainer's patch-note draft."""

import re
import sys
from pathlib import Path


def public_notes(text):
    lines = text.splitlines(keepends=True)
    if lines and lines[0].startswith('# '):
        lines = lines[1:]
    public = []
    private_level = None
    for line in lines:
        heading = re.match(r'^(#{1,6})\s+(.+?)\s*#*\s*$', line)
        if heading:
            level = len(heading[1])
            if private_level is not None and level <= private_level:
                private_level = None
            if heading[2] == 'For the website (SDK guide)':
                private_level = level
        if private_level is None:
            public.append(line)
    body = ''.join(public).strip()
    return body + '\n' if body else ''


if __name__ == '__main__':
    sys.stdout.write(public_notes(Path(sys.argv[1]).read_text(encoding='utf-8')))
