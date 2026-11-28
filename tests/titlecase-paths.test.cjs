const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

// Runs titlecase.js on its own and hands back the function the launcher uses on its words.
const titleCase = () => {
  const src = fs.readFileSync(path.join(__dirname, '../ui/titlecase.js'), 'utf8');
  const window = {};
  const document = { body: null, documentElement: { lang: 'en' }, addEventListener() {} };
  vm.runInNewContext(src, { window, document, MutationObserver: class { observe() {} } });
  return window.titleCase;
};

test('the launcher\'s own words are still in Title Case', () => {
  assert.equal(titleCase()('open its folder'), 'Open Its Folder');
});

test('a Linux log path stays as it is', () => {
  const shown = 'From /home/ruben/.local/share/RecompLauncher/logs/game.log';
  assert.equal(titleCase()(shown), 'From /home/ruben/.local/share/RecompLauncher/logs/game.log');
});

test('a Windows path stays as it is', () => {
  assert.equal(titleCase()('From C:\\games\\save\\log.txt'), 'From C:\\games\\save\\log.txt');
});

test('a web address stays as it is', () => {
  assert.equal(titleCase()('open github.com/dev/game'), 'Open github.com/dev/game');
});

test('"back" is one of the launcher\'s own words', () => {
  assert.equal(titleCase()('Jump back in'), 'Jump Back In');
});

test('"available" is one of the launcher\'s own words', () => {
  assert.equal(titleCase()('update available'), 'Update Available');
});
