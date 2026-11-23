const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');

test('a finished install forgets the project support read from the old files', () => {
  const from = app.indexOf("if (p.phase === 'done') {");
  assert.ok(from >= 0);
  const done = app.slice(from, app.indexOf('loadNews()', from));
  assert.match(done, /delete S\.supLocal\[id\]/);
});
