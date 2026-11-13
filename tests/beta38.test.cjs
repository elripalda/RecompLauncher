const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const read = (f) => fs.readFileSync(path.join(__dirname, '../ui', f), 'utf8');
const app = read('app.js'), tv = read('tv.js'), ach = read('achievements.js'), css = read('app.css'), account = read('account.js');

// A const arrow function out of achievements.js, by name, to run on its own.
function arrow(src, name) {
  const start = src.indexOf(`const ${name} = `);
  assert.ok(start >= 0, name);
  let i = src.indexOf('=>', start), depth = 0;
  for (; i < src.length; i++) {
    const ch = src[i];
    if (ch === '{' || ch === '(') depth++;
    if (ch === '}' || ch === ')') depth--;
    if (ch === ';' && depth === 0) break;
  }
  return src.slice(start, i + 1);
}

test('a game list never leaves one card alone on its last row', () => {
  const evenCols = new Function(`${arrow(ach, 'evenCols')} return evenCols;`)();
  for (let n = 1; n <= 40; n++) {
    for (const max of [4, 5]) {
      const c = evenCols(n, max);
      assert.ok(c >= 1 && c <= max, `${n} in ${max}: ${c}`);
      // With four columns at most, 13, 25 and 37 can't be split without one left over.
      if (n > 1 && !(max === 4 && n % 12 === 1)) assert.notEqual(n % c, 1, `${n} cards in ${c} columns leaves one alone`);
    }
  }
  assert.equal(evenCols(5, 5), 5);
  assert.equal(evenCols(6, 5), 3);
  assert.equal(evenCols(7, 5), 4);
});

test('rarity reads a fraction or a percentage, and nothing when there is no rate', () => {
  const t = (x) => x, tf = (s, v) => s.replace(/\{(\w+)\}/g, (_, k) => v[k]);
  const rarityText = new Function('t', 'tf', `${arrow(ach, 'rarityText')} return rarityText;`)(t, tf);
  assert.equal(rarityText(0.125), '13% of players');
  assert.equal(rarityText(12.5), '13% of players');
  assert.equal(rarityText(0.004), '0.4% of players');
  assert.equal(rarityText(0), 'Nobody yet');
  assert.equal(rarityText(null), '');
  assert.match(ach, /const rate = \(\(mine\.rarity \|\| \{\}\)\[id\] \|\| \{\}\)\.rate;\r?\n\s*const rare = rate != null \? rarityText\(rate\) : '';/);
});

test('the game page title has the whole width above the details and buttons', () => {
  assert.match(app, /<div class="gp-name">.*<h1 class="\$\{a\.name\.length > 34 \? 'long' : ''\}">/);
  assert.match(css, /grid-template-areas:"cover name name" "cover title side"/);
  // With less room, the buttons go on a row of their own instead of squeezing the title.
  assert.match(css, /@container gp \(max-width:1260px\)\{[^}]*"cover name" "cover title" "cover side"/);
  assert.doesNotMatch(css, /\.gp-title h1\{/);
});

test('the left stick can be left out of moving around, on the desktop and in Big Picture', () => {
  assert.match(app, /sw\('pad_stick', st\.pad_stick !== false/);
  assert.match(tv, /sw\('pad_stick', t\('Move with the left stick'\)/);
});

test('system icons are vector images', () => {
  assert.match(app, /--m:url\(img\/\$\{k\}\.svg\)/);
  for (const k of ['windows', 'linux', 'macos']) assert.ok(fs.existsSync(path.join(__dirname, '../ui/img', `${k}.svg`)), k);
});

test('the Saves page never asks again and again for an empty answer', () => {
  assert.match(account, /S\.savesAll = \(await invoke\('saves_overview'\)\) \|\| \{ games: \[\] \}/);
});
