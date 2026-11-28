const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const read = (f) => fs.readFileSync(path.join(__dirname, '../ui', f), 'utf8');

// i18n.js in German, its text read from the real de.json.
async function german() {
  const window = {};
  const de = JSON.parse(read('i18n/de.json'));
  const fetch = async () => ({ ok: true, json: async () => de });
  const document = { documentElement: {} };
  vm.runInNewContext(read('i18n.js'), { window, navigator: { languages: ['de-DE'] }, document, fetch });
  await window.loadLang('de');
  return { t: window.t, de };
}

const TOO_LONG = "A path in the game's folder is too long for Windows. Add a library folder with a shorter path in Settings → Games and Folders and move the game there, or turn on Windows' long paths setting (LongPathsEnabled), then try again. The launcher doesn't change that setting for you.";

test('a launcher text with a program line under it is translated, and the program line kept', async () => {
  const { t, de } = await german();
  assert.equal(t(`${TOO_LONG}\nerror: out of disk space`), `${de[TOO_LONG]}\nerror: out of disk space`);
});

test('a text whose first line the launcher does not know stays as it is', async () => {
  const { t } = await german();
  assert.equal(t('something a program said\nand more'), 'something a program said\nand more');
});

test('the desktop setup error keeps its lines apart', () => {
  assert.match(read('app.css'), /\.su-err\{[^}]*white-space:pre-line/);
});
