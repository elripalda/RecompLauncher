const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const fn = (name) => app.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];

// An installed game on its own page, with the CRT shader picked. The production refreshGame,
// gameTitle and lookChip run; everything else on the page is a stub that draws nothing.
const id = 'mm-recomp';
function gamePage() {
  const drawn = {};
  const el = (sel) => ({ sel, dataset: { gp: id }, classList: { add: () => {} } });
  const Looks = { defs: () => ({ looks: [{ id: 'crt', name: 'CRT' }, { id: 'sharp', name: 'Sharp' }] }) };
  const c = vm.createContext({
    S: { view: 'game', arg: id, tab: 'looks', library: { [id]: { id, look: 'crt' } }, setup: {}, settings: {} },
    window: { Looks },
    Looks,
    looksHere: () => true,
    t: (x) => x,
    tf: (x, o) => x.replace('{name}', o.name),
    esc: (x) => String(x),
    $: (sel) => el(sel),
    $$: () => [],
    morph: (target, html) => { drawn[target.sel] = html; },
    byId: () => ({ id, name: 'Majora\'s Mask' }),
    gameTabs: () => [],
    actions: () => '',
    gameBody: () => '',
    statePill: () => '',
    tagsOf: () => [],
    ...Object.fromEntries(['reviveSoon', 'moveTabPill', 'paintLooks', 'fitTitles', 'layoutShelves', 'render', 'loadSetup'].map((k) => [k, () => {}])),
    ...Object.fromEntries(['turnkey', 'greatOnDeck', 'greatHandheld', 'isAI'].map((k) => [k, () => false])),
    ...Object.fromEntries(['partOfLine', 'based', 'devLine', 'verPill'].map((k) => [k, () => ''])),
  });
  for (const name of ['refreshGame', 'gameTitle', 'lookChip']) vm.runInContext(fn(name), c);
  return { c, drawn };
}

test('the line under the game name follows a shader picked on the page', () => {
  const { c, drawn } = gamePage();
  c.S.library[id].look = 'sharp';
  c.refreshGame();
  assert.match(drawn['.gp-title'], /Sharp shader/);
  assert.doesNotMatch(drawn['.gp-title'], /CRT shader/);
});
