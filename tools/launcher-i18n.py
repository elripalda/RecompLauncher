#!/usr/bin/env python3
"""The launcher's text, for translating.

  python3 tools/launcher-i18n.py              how much of each language is done
  python3 tools/launcher-i18n.py --strings    every English text, as JSON (plural ones marked)
  python3 tools/launcher-i18n.py --missing de the texts German doesn't have yet, as JSON
  python3 tools/launcher-i18n.py --check      checks every language file: valid JSON, the same
                                              {placeholders}, the right number of plural forms,
                                              and texts the launcher no longer uses

The English comes from t('…'), tf('…'), tn(n, '…', '…') and N_('…') in ui/*.js, elements marked
data-t in ui/*.html, the looks in ui/looks.json and the fixed messages the core shows (Rust string
literals given back as errors). A translation goes in ui/i18n/<code>.json as {"English": "text"}.
Texts with {n} in them take one form per count in Polish, Russian and Ukrainian
("one|few|many") and Arabic ("two|few|many|other"); other languages give a single form.
"""
import html.parser
import json
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
UI = os.path.join(ROOT, 'ui')
CODES = ['en-GB', 'es', 'es-419', 'fr', 'fr-CA', 'pt', 'pt-BR', 'it', 'de', 'nl', 'pl', 'tr', 'ru', 'uk',
         'vi', 'id', 'th', 'ar', 'ja', 'zh-Hans', 'zh-Hant', 'ko']
BASE = {'en-GB': 'en', 'es-419': 'es', 'fr-CA': 'fr', 'pt-BR': 'pt'}
FORMS = {'pl': 3, 'ru': 3, 'uk': 3, 'ar': 4}

LIT = r"""('(?:[^'\\\n]|\\.)*'|"(?:[^"\\\n]|\\.)*")"""


def unquote(s):
    q = s[0]
    body = s[1:-1]
    return re.sub(r'\\(.)', lambda m: {'n': '\n', 't': '\t'}.get(m.group(1), m.group(1)), body) if q in '\'"' else body


def from_js(text, out, plural):
    for m in re.finditer(r'(?<![\w.$])(?:t|tf|N_)\(\s*' + LIT, text):
        out.add(unquote(m.group(1)))
    for m in re.finditer(r'(?<![\w.$])tn\([^\n]*?,\s*' + LIT + r'\s*,\s*' + LIT + r'\s*\)', text):
        out.add(unquote(m.group(1)))
        out.add(unquote(m.group(2)))


class Marked(html.parser.HTMLParser):
    VOID = {'br', 'img', 'input', 'meta', 'link', 'hr', 'source'}

    def __init__(self, out):
        super().__init__()
        self.out = out
        self.stack = []

    def handle_starttag(self, tag, attrs):
        a = dict(attrs)
        marked = 'data-t' in a
        if marked:
            for k in ('title', 'aria-label', 'placeholder'):
                if a.get(k):
                    self.out.add(a[k])
        if tag not in self.VOID:
            self.stack.append(marked)

    def handle_endtag(self, tag):
        if tag not in self.VOID and self.stack:
            self.stack.pop()

    def handle_data(self, data):
        if self.stack and self.stack[-1] and data.strip():
            self.out.add(data.strip())


def rust(text, out):
    for m in re.finditer(r'"([A-Z][^"{}\\]{6,}[.?!…])"\s*\.(?:into|to_string|to_owned)\(\)', text):
        out.add(m.group(1))
    for m in re.finditer(r'(?:Err|ok_or|map_err\(\|_\|)\(?\s*"([A-Z][^"{}\\]{6,}[.?!…])"', text):
        out.add(m.group(1))


def strings():
    out, plural = set(), set()
    for f in sorted(os.listdir(UI)):
        p = os.path.join(UI, f)
        if f.startswith('_') or f == 'i18n.js':
            continue
        if f.endswith('.js'):
            from_js(open(p, encoding='utf-8').read(), out, plural)
        elif f.endswith('.html'):
            Marked(out).feed(open(p, encoding='utf-8').read())
    looks = json.load(open(os.path.join(UI, 'looks.json'), encoding='utf-8'))
    for k in looks.get('looks', []):
        for field in ('name', 'blurb', 'amount_label', 'about'):
            if isinstance(k.get(field), str) and k[field]:
                out.add(k[field])
    src = os.path.join(ROOT, 'src-tauri', 'src')
    for f in sorted(os.listdir(src)):
        if f.endswith('.rs'):
            rust(open(os.path.join(src, f), encoding='utf-8').read(), out)
    out.discard('')
    # Any text with a count in it can take plural forms: tf() and tn() pick the form by {n}.
    plural |= {x for x in out if '{n}' in x}
    return sorted(out), plural


def load(code):
    p = os.path.join(UI, 'i18n', code + '.json')
    if not os.path.exists(p):
        return {}
    return json.load(open(p, encoding='utf-8'))


def holes(s):
    return sorted(set(re.findall(r'\{(\w+)\}', s)))


def check():
    eng, plural = strings()
    have = set(eng)
    bad = 0
    for code in CODES:
        try:
            d = load(code)
        except ValueError as e:
            print(f'{code}: not valid JSON: {e}')
            bad += 1
            continue
        base = BASE.get(code, code)
        for en, tr in d.items():
            if not isinstance(tr, str):
                print(f'{code}: not text: {en!r}')
                bad += 1
                continue
            if en not in have:
                print(f'{code}: no longer used: {en!r}')
            parts = tr.split('|')
            want = FORMS.get(base, 1) if en in plural else 1
            if '|' in en:
                want = len(en.split('|'))
            if len(parts) != want and not (en in plural and base == 'ar' and len(parts) == 1):
                print(f'{code}: {len(parts)} forms, wants {want}: {en!r} -> {tr!r}')
                bad += 1
            for p in parts:
                if holes(p) != holes(en) and not (en in plural and holes(p) == []):
                    print(f'{code}: placeholders {holes(p)} vs {holes(en)}: {en!r} -> {tr!r}')
                    bad += 1
    return bad


def main():
    a = sys.argv[1:]
    eng, plural = strings()
    if a[:1] == ['--strings']:
        print(json.dumps([{'en': s, 'plural': True} if s in plural else {'en': s} for s in eng], ensure_ascii=False, indent=1))
    elif a[:1] == ['--missing'] and len(a) > 1:
        d = load(a[1])
        if a[1] in BASE and BASE[a[1]] != 'en':
            d = {**load(BASE[a[1]]), **d}
        print(json.dumps([{'en': s, 'plural': True} if s in plural else {'en': s} for s in eng if s not in d], ensure_ascii=False, indent=1))
    elif a[:1] == ['--check']:
        sys.exit(1 if check() else 0)
    else:
        print(f'{len(eng)} texts ({len(plural)} with plural forms)')
        for code in CODES:
            d = load(code)
            if code in BASE and BASE[code] != 'en':
                d = {**load(BASE[code]), **d}
            n = sum(1 for s in eng if s in d)
            print(f'  {code:8} {n:5} / {len(eng)}')


if __name__ == '__main__':
    main()
