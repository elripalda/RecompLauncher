const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');

function fn(name) {
  const start = app.indexOf(`function ${name}(`);
  assert.ok(start >= 0, name);
  return app.slice(start, app.indexOf('\n}', start) + 2);
}

// Home with one featured recomp and one played game. Everything pageHome leans on is a plain
// stand-in, so the page is just the banner marker followed by the shelf keys.
function home(ui) {
  const a = { id: 'game', name: 'Game', featured: true, hero: '/hero.jpg' };
  const list = () => [a];
  const none = () => [];
  const same = (x) => x;
  const c = vm.createContext({
    S: { settings: { ui }, library: { game: { id: 'game' } }, easy: new Set() },
    visible: list, forOS: () => true, bannerApps: list, turnkeyFirst: same, oneEach: same, uniq: same,
    notPC: () => true, byDl: () => 0, byNew: () => 0, byId: () => a, withGames: same, turnkey: () => false, easyTag: () => false,
    jumpRecs: list, libApp: same, foundInFolders: none, haveAny: () => false, favs: none, follows: none,
    recentlyUpdated: none, devOf: () => null, foundSection: () => '', foundCallout: () => '',
    trendShelf: () => '', newsSection: () => '', offlineNote: () => '', sponsBand: () => '',
    t: same, tf: same, esc: same, ago: same,
    homeBanner: () => '', homeBox: () => '',
    slider: () => '[banner]',
    shelf: (key) => `[${key}]`
  });
  vm.runInContext(fn('pageHome'), c);
  vm.runInContext(app.slice(app.indexOf('const HOME_ROWS'), app.indexOf('\n', app.indexOf('const HOME_ROWS'))), c);
  vm.runInContext(fn('homePlan') + fn('homeLayout'), c);
  return vm.runInContext('pageHome()', c).replace(/\s+/g, '');
}

test('Home opens with the featured banner by default', () => {
  assert.match(home({}), /^<divclass="pagehome">\[banner\]\[played\]/);
});

test('Home starts at Jump back in when the banner is turned off', () => {
  assert.match(home({ hide_hero: true }), /^<divclass="pagehome">\[played\]/);
});

test('Settings → Appearance has the banner switch, on while the banner shows', () => {
  assert.match(fn('pageSettings'), /sw\('hide_hero', !ui\.hide_hero, t\('Featured Banner on Home'\), 'ui-toggle'\)/);
});
