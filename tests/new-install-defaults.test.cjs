const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const fn = (src, name) => src.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];

// newInstallDefaults at start, on a screen of the given size, with these settings.
function startOn(width, height, settings = { ui: {} }) {
  const saved = [];
  const c = vm.createContext({
    S: { settings },
    screen: { width, height },
    saveUi: (patch) => { saved.push(patch); },
  });
  vm.runInContext(fn(app, 'bannerTooBig') + '\n' + fn(app, 'newInstallDefaults'), c);
  c.newInstallDefaults();
  return saved;
}
const bannerOff = (saved) => saved.some((p) => p.hide_hero === true);

test('a new install on a 1366×768 screen starts with the Featured Banner off', () => {
  assert.equal(bannerOff(startOn(1366, 768)), true);
});

test('a new install on 1080p at 125% scaling starts with the Featured Banner off', () => {
  assert.equal(bannerOff(startOn(1536, 864)), true);
});

test('a new install on a 1080p screen keeps the Featured Banner', () => {
  assert.equal(bannerOff(startOn(1920, 1080)), false);
});

test('running setup again on a small screen leaves the Featured Banner as it was', () => {
  const settings = { setup_done: false, ui: { seen_version: '0.90.0-beta.68' } };
  assert.equal(bannerOff(startOn(1366, 768, settings)), false);
});

test('a player who has used the launcher keeps the Featured Banner on a small screen', () => {
  const settings = { setup_done: true, ui: {} };
  assert.equal(bannerOff(startOn(1366, 768, settings)), false);
});

test('the new install defaults are chosen at start, once the settings have loaded', () => {
  assert.match(app, /if \(loaded\) \{[^\n]*newInstallDefaults\(\);/);
});
