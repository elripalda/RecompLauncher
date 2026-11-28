const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const tv = fs.readFileSync(path.join(__dirname, '../ui/tv.js'), 'utf8');
const fn = name => app.match(new RegExp(`^function ${name}\\([^]*?^}`, 'm'))[0];

function render(a) {
  const c = vm.createContext({
    S: { site: 'https://recomplauncher.com', thumbs: 'https://recomplauncher.com/api/thumb.php',
      library: {}, mods: { game: [] }, settings: {}, setup: {}, slide: 0 },
    T: { core: {} }, LOADED: new Set(), window: {}, GENRES: {}, CHEV_L: '', CHEV_R: '',
    t: s => s, byId: () => a, bannerInfo: () => ({}), gameTabs: () => [],
    fileSrc: p => 'asset:' + p,
    ...Object.fromEntries(['wantCounts', 'gameTitle', 'actions', 'replacedNote', 'betterNote',
      'reinstallNote', 'badNote', 'stateNote', 'soonNote', 'igCard', 'includesBox', 'gameBody',
      'slideTitle', 'based', 'slideActs', 'originName'].map(k => [k, () => '']))
  });
  for (const name of ['esc', 'webUrl', 'THUMB_RE', 'THUMB_W', 'thumbW', 'thumbOf', 'media', 'pic']) {
    vm.runInContext(app.match(new RegExp(`^const ${name} = .*`, 'm'))[0], c);
  }
  vm.runInContext(fn('pageGame') + fn('slider'), c);
  vm.runInContext(tv.match(/^  const heroOf = .*/m)[0], c);
  c.a = a;
  return {
    page: c.pageGame('game'), slider: c.slider([a]),
    tv: vm.runInContext('heroOf(a)', c)
  };
}
const game = { id: 'game', name: 'Game', hero: '/media/game-hero.jpg', cover: '/media/game-cover.jpg' };

test('desktop game backgrounds use the original art while the cover stays a thumbnail', () => {
  const { page } = render(game);
  assert.match(page, /class="gp-hero"><img[^>]+src="https:\/\/recomplauncher.com\/media\/game-hero.jpg"/);
  assert.match(page, /class="gp-cover"><img[^>]+w=400/);
});

test('Home banners use the original art while the small cover stays a thumbnail', () => {
  const { slider } = render(game);
  assert.match(slider, /src="https:\/\/recomplauncher.com\/media\/game-hero.jpg"[^>]+class="bg"/);
  assert.match(slider, /class="sl-cover"[^]*w=400/);
});

test('Big Picture uses the original background instead of a 960-pixel copy', () => {
  assert.equal(render(game).tv, 'https://recomplauncher.com/media/game-hero.jpg');
});

test('a missing background falls back to the original cover', () => {
  const a = { ...game, hero: '' };
  assert.equal(render(a).tv, 'https://recomplauncher.com/media/game-cover.jpg');
  assert.match(render(a).page, /class="gp-hero"><img[^>]+src="https:\/\/recomplauncher.com\/media\/game-cover.jpg"/);
});

test('custom local background paths still use the local file protocol', () => {
  const a = { ...game, hero: 'local:/pictures/hero.jpg' };
  assert.equal(render(a).tv, 'asset:/pictures/hero.jpg');
  assert.match(render(a).page, /class="gp-hero"><img[^>]+src="asset:\/pictures\/hero.jpg"/);
});
