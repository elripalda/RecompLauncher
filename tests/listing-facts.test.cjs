// The player's note, multiplayer and server are facts on the game's listing. The store moves them
// out of its old-style boot profiles, so a game without one still shows them; a catalog from before
// still carries them in `boot`.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const line = (name) => app.match(new RegExp(`^const ${name} = .*$`, 'm'))[0];
const facts = new Function(`${['listingFact', 'isServer', 'isMulti', 'bootNote'].map(line).join('\n')} return { isServer, isMulti, bootNote };`)();

test('the listing\'s own facts are read first', () => {
  const a = { id: 'g', player_note: 'Pick your USA ROM.', multiplayer: 'local', server: true, boot: { player_note: 'old', multiplayer: false } };
  assert.equal(facts.bootNote(a), 'Pick your USA ROM.');
  assert.equal(facts.isMulti(a), true);
  assert.equal(facts.isServer(a), true);
});

test('a catalog from before still has them in the boot profile', () => {
  const a = { id: 'g', boot: { player_note: 'Answer Yes to its questions.', multiplayer: 'local', server: true } };
  assert.equal(facts.bootNote(a), 'Answer Yes to its questions.');
  assert.equal(facts.isMulti(a), true);
  assert.equal(facts.isServer(a), true);
});

test('without either, there is nothing to show', () => {
  assert.equal(facts.bootNote({ id: 'g' }), '');
  assert.equal(facts.isMulti({ id: 'g', tags: [] }), false);
  assert.equal(facts.isServer({ id: 'g' }), false);
});
