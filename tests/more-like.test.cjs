const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');

const source = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
// More Like This on a game page, with the catalog stubbed.
function moreLike(a, list) {
  const ctx = vm.createContext({ visible: () => list, forOS: () => true, tagsOf: (x) => x.tags || [], a });
  vm.runInContext(source.match(/^function moreLike\([^]*?^}/m)[0], ctx);
  return vm.runInContext('moreLike(a)', ctx).map((x) => x.id);
}

test('More Like This shows each game once, its best-matching version', () => {
  const a = { id: 'oot', title: 'oot', genre: 'Adventure', tags: ['3d'] };
  const list = [
    { id: 'poko-a', title: 'pokopia', genre: 'Adventure', downloads: 10 },
    { id: 'poko-b', title: 'pokopia', genre: 'Adventure', tags: ['3d'], downloads: 5 },
    { id: 'poko-c', title: 'pokopia', genre: 'Adventure', downloads: 99 },
    { id: 'mkw', title: 'mkw', genre: 'Adventure', downloads: 1 },
    { id: 'oot-2', title: 'oot', genre: 'Adventure' },
  ];
  assert.deepEqual(moreLike(a, list), ['poko-b', 'mkw']);
});
