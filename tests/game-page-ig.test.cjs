const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const fn = (src, name) => src.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];
const same = (x) => x;

// igCard and igSlot with a stand-in entry placed where the site says.
const cardAt = (at) => {
  const run = new Function('igEntry', 'esc', 't', 'fmtNum', 'media', 'SITE', `const IG_PLACES = ['side', 'strip', 'play']; const igPlace = (e) => (IG_PLACES.includes(e && e.at) ? e.at : 'side'); ${fn(app, 'igCard')}; ${fn(app, 'igSlot')}; return { igCard, igSlot };`);
  return run(() => ({ t: 'Game', p: 'Switch', price: '$10', at }), same, same, String, same, '');
};

test('the store card is never next to Play', () => {
  const side = app.match(/<div class="gp-side">[^\n]*/)[0];
  assert.doesNotMatch(side, /ig-slot|igCard/);
});

test('each place shows the card only where the site put it', () => {
  const g = { id: 'g' };
  const side = cardAt('side'), strip = cardAt('strip'), play = cardAt('play');
  assert.match(side.igCard(g, 'side'), /ig-card/);
  assert.equal(side.igCard(g, 'strip'), '');
  assert.match(strip.igCard(g, 'strip'), /ig-card ig-wide/);
  assert.equal(strip.igCard(g, 'side'), '');
  assert.match(play.igCard(g, 'play'), /ig-wide/);
  // No play time yet: a card meant for under it shows under the stats.
  assert.match(play.igCard(g, 'strip', true), /ig-wide/);
  assert.equal(play.igCard(g, 'strip', false), '');
  // Anything else the site sends: the right column.
  assert.match(cardAt('beside-play').igCard(g, 'side'), /ig-card/);
});

test('play times are short in English', () => {
  const run = new Function('t', 'fmtNum', 'fmtTime', 'document', `${app.match(/^const fmtTimeShort = [^]*?^};/m)[0]}; return fmtTimeShort;`);
  const short = run(same, String, () => 'long', { documentElement: { lang: 'en' } });
  assert.equal(short(2 * 3600 + 8 * 60), '2 hrs 8 min');
  assert.equal(short(3600), '1 hr');
  assert.equal(short(46 * 60), '46 min');
  assert.equal(short(30), '1 min');
  const de = run(same, String, () => 'long', { documentElement: { lang: 'de' } });
  assert.equal(de(7200), 'long');
});
