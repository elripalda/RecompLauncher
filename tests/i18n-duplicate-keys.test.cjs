const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const DIR = path.join(__dirname, '../ui/i18n');

// The top-level keys of a JSON object, in order, repeats kept. JSON.parse keeps only the last
// copy of a repeated key, so the text is scanned: a string right after `{` or `,` at depth 1 is a key.
function topKeys(text) {
  const keys = [];
  let depth = 0;
  let wantKey = false;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (c === '"') {
      let j = i + 1;
      while (text[j] !== '"') j += text[j] === '\\' ? 2 : 1;
      if (depth === 1 && wantKey) keys.push(JSON.parse(text.slice(i, j + 1)));
      wantKey = false;
      i = j;
    } else if (c === '{' || c === '[') {
      depth++;
      wantKey = c === '{' && depth === 1;
    } else if (c === '}' || c === ']') {
      depth--;
    } else if (c === ',') {
      wantKey = depth === 1;
    }
  }
  return keys;
}

test('the key scanner sees a repeated key, and escaped quotes inside a text', () => {
  assert.deepEqual(topKeys('{"a \\"b\\"": "x \\" ,\\"c\\":", "d": {"e": 1}, "a \\"b\\"": "y"}'), ['a "b"', 'd', 'a "b"']);
});

test('no language file has the same text twice', () => {
  const files = fs.readdirSync(DIR).filter((f) => f.endsWith('.json'));
  assert.ok(files.length > 0);
  const found = [];
  for (const f of files) {
    const seen = new Set();
    for (const k of topKeys(fs.readFileSync(path.join(DIR, f), 'utf8'))) {
      if (seen.has(k)) found.push(`${f}: ${JSON.stringify(k)}`);
      seen.add(k);
    }
  }
  assert.deepEqual(found, []);
});
