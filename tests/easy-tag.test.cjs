// The Easy Setup tag, filter, rows and sorting go by the core's answer (game_support): the
// project's own file, or our profile turned on for the game by hand. An older profile of ours
// still sets the game up the same way, untagged. Desktop and Big Picture show the same answer.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const tv = fs.readFileSync(path.join(__dirname, '../ui/tv.js'), 'utf8');
const line = (name) => app.match(new RegExp(`^const ${name} = .*$`, 'm'))[0].replace(/^const /, 'globalThis.');
const fn = (name) => app.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];

function context(sup, os = 'windows') {
  const c = vm.createContext({ S: { os, library: {}, supLocal: {}, sup, settings: { ui: {} } }, needsSetup: () => false, render: () => {} });
  vm.runInContext([line('supLocalKey'), fn('supOf'), fn('applyEasySkip'), line('feedsCopy'), line('turnkey'), line('easyTag'), line('turnkeyFirst')].join('\n'), c);
  return c;
}
// What loadEasy does with the core's answer.
const loaded = (sup, os) => {
  const c = context(sup, os);
  c.invoke = async () => sup;
  vm.runInContext(fn('loadEasy'), c);
  return vm.runInContext('loadEasy(true)', c).then(() => c);
};
const SUP = {
  theirs: { from: 'release', easy: true, tag: true, easy_setup: 'p' },
  ours: { from: 'ours', easy: true, tag: true, easy_setup: 'l' },
  older: { from: 'store', easy: true, tag: false, easy_setup: 'l' },
  settings: { from: 'release', easy: false, tag: false, settings: 'p' },
};

test('the core\'s Easy Setup is what Install sets up: tagged or not', async () => {
  const c = await loaded(SUP);
  assert.deepEqual([...c.S.easyAll].sort(), ['older', 'ours', 'theirs']);
  for (const id of ['theirs', 'ours', 'older']) assert.equal(c.turnkey({ id }), true, id);
  assert.equal(c.turnkey({ id: 'settings' }), false);
});

test('the project\'s own file and our profile turned on by hand are tagged; an older profile of ours is not', async () => {
  const c = await loaded(SUP);
  assert.equal(c.easyTag({ id: 'theirs' }), true);
  assert.equal(c.easyTag({ id: 'ours' }), true);
  assert.equal(c.easyTag({ id: 'older' }), false);
  assert.equal(c.easyTag({ id: 'settings' }), false);
  assert.equal(c.easyTag({ id: 'plain' }), false);
});

test('a tag on a game the launcher doesn\'t set up (skipped, or on Android) is not shown', async () => {
  const c = await loaded(SUP, 'android');
  assert.equal(c.easyTag({ id: 'theirs' }), false);
  const d = await loaded(SUP);
  d.S.settings.ui.easy_skip = ['theirs'];
  d.applyEasySkip();
  assert.equal(d.easyTag({ id: 'theirs' }), false);
});

test('a game of a recomp of several follows its recomp', async () => {
  const c = await loaded(SUP);
  assert.equal(c.easyTag({ id: 'theirs--one', parent: 'theirs' }), true);
  assert.equal(c.easyTag({ id: 'older--one', parent: 'older' }), false);
});

test('tagged games come first, nothing left out', async () => {
  const c = await loaded(SUP);
  assert.deepEqual(Array.from(c.turnkeyFirst([{ id: 'older' }, { id: 'plain' }, { id: 'ours' }])).map((a) => a.id), ['ours', 'older', 'plain']);
});

test('the window no longer guesses Easy Setup from boot profiles or files', () => {
  assert.doesNotMatch(app, /TURNKEY_HOW|builtinBoot|function fileCovers|invoke\('easy_setup'\)/);
  assert.doesNotMatch(tv, /TURNKEY_HOW|builtinBoot/);
});

test('Big Picture\'s card badge, game page pill, Discover chip and filter, and both Home rows go by the tag', () => {
  const tvPoster = tv.match(/  function poster\(a, key, sub, rank\) \{[^]*?\n  \}/)[0];
  assert.match(tvPoster, /!S\.library\[a\.id\] && easyTag\(a\) \?/);
  assert.match(tv, /: easyTag\(a\) \? `<span class="tv-easy">\$\{esc\(t\('Easy Setup'\)\)\}<\/span>`/);
  assert.match(tv, /apps\.some\(easyTag\) \? \[\['easy', N_\('Easy Setup'\)\]\]/);
  assert.match(tv, /g === 'easy' \? easyTag\(a\)/);
  assert.ok(tv.includes('games.filter(easyTag).sort(byNew)'));
  const dropIn = 'games.filter((a) => S.easy.has(a.parent || a.id) && easyTag(a) && !haveAny(a))';
  assert.ok(fn('pageHome').includes(dropIn));
  assert.ok(tv.includes(dropIn));
  assert.ok(fn('pageHome').includes('games.filter(easyTag).sort(byNew)'));
  assert.match(app, /\(!f\.easy \|\| easyTag\(a\)\)/);
});

test('what Install and Play do still goes by turnkey, not the tag', () => {
  assert.match(app, /turnkey\(a\) \? b\('main easy', 'install', esc\(t\('Easy Setup'\)\)/);
  assert.match(app, /const askCopy = !selfRun && !building && feedsCopy\(a\);/);
  assert.match(tv, /esc\(turnkey\(a\) \? t\('Easy Setup'\) : t\('Install'\)\)/);
});
