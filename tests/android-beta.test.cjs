// The Android app after its first beta test: Big Picture made for a phone, settings for older
// phones and for easier reading, the start-up screen, the icon, and lists that load as they scroll.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const read = (f) => fs.readFileSync(path.join(__dirname, '..', f), 'utf8');
const exists = (f) => fs.existsSync(path.join(__dirname, '..', f));
const phone = read('ui/phone.js'), css = read('ui/phone.css'), tv = read('ui/tv.js'), app = read('ui/app.js');
const RES = 'src-tauri/gen/android/app/src/main/res';

test('a phone opens its own Big Picture mode; a tablet, a handheld and Android TV get the big-screen one', () => {
  const line = tv.split('\n').find((l) => l.includes('const phoneBp = '));
  const fn = app.match(/^function droidPhone\(\) \{[^]*?^\}/m)[0];
  const pick = (touch, os, dev, short = 393) => vm.runInNewContext(`${fn}; ${line}; phoneBp()`, { S: { droidDev: dev }, screen: { width: short, height: 873 }, android: () => os === 'android', touchHere: () => touch, window: { phoneBp: { name: 'phone' } } });
  assert.deepEqual(pick(true, 'android', { sw: 393, pad: false }), { name: 'phone' });
  assert.deepEqual(pick(true, 'android', null), { name: 'phone' });
  assert.equal(pick(true, 'android', { sw: 800, pad: false }), null);
  assert.equal(pick(true, 'android', null, 800), null);
  assert.equal(pick(true, 'android', { sw: 411, pad: true }), null);
  assert.equal(pick(false, 'android', { sw: 393 }), null);
  assert.equal(pick(true, 'windows', { sw: 393 }), null);
  // Every way in goes through enter(): Settings, the start-up choice, a controller, F11.
  assert.match(tv, /async function enter\(\) \{\n    if \(phoneBp\(\)\) return phoneBp\(\)\.enter\(padHere\(\)\);/);
  // The launcher asks Android what it runs on before it picks a layout.
  assert.match(app, /S\.droidDev = await invoke\('android_device'\)\.catch\(\(\) => null\);\n[^]{0,400}?\n    const open = startsInTV\(\)/);
  // Settings, Big Picture style: the player's pick wins over what the device is.
  assert.deepEqual(vm.runInNewContext(`${fn}; ${line}; phoneBp()`, { S: { droidDev: { sw: 800 }, settings: { ui: { bp_style: 'phone' } } }, screen: { width: 800, height: 1280 }, android: () => true, touchHere: () => true, window: { phoneBp: { name: 'phone' } } }), { name: 'phone' });
  assert.equal(vm.runInNewContext(`${fn}; ${line}; phoneBp()`, { S: { droidDev: { sw: 393 }, settings: { ui: { bp_style: 'full' } } }, screen: { width: 393, height: 873 }, android: () => true, touchHere: () => true, window: { phoneBp: { name: 'phone' } } }), null);
});

test('the phone\'s Big Picture mode answers the controller, Android\'s Back and the last controller leaving', () => {
  assert.match(phone, /window\.phoneBp = \{ enter: bpEnter, leave: bpLeave, on: \(\) => !!bp, pad: bpPad, padGone: bpPadGone, back: bpSysBack \};/);
  assert.match(tv, /if \(window\.phoneBp && window\.phoneBp\.on\(\)\) return window\.phoneBp\.back\(\);/);
  assert.match(tv, /else if \(phoneBp\(\)\) phoneBp\(\)\.padGone\(\);/);
  // Unplugging leaves only when a controller is what opened it, and not when it's how the launcher starts.
  assert.match(phone, /if \(bp && bp\.byPad && !S\.settings\.start_in_tv\) bpLeave\(\);/);
});

test('the phone\'s Big Picture mode turns sideways and hides the system bars, and puts them back on the way out', () => {
  const enter = phone.slice(phone.indexOf('function bpEnter('), phone.indexOf('function bpLeave('));
  assert.match(enter, /android_orientation', \{ landscape: true \}/);
  assert.match(enter, /android_edges', \{ phone: false, light: false \}/);
  const leave = phone.slice(phone.indexOf('function bpLeave('), phone.indexOf('function bpPadGone('));
  assert.match(leave, /android_orientation', \{ landscape: false \}/);
  assert.match(leave, /delete root\.dataset\.theme;\n    applyLook\(\);/);
});

test('Big Picture mode is always dark, whatever the phone layout\'s theme', () => {
  assert.match(css, /\.ph-bp,#phone\.ph-bpon \.ph-sheet,#phone\.ph-bpon \.ph-scrim\{--bg:#0B1220;/);
  assert.match(css, /#phone\.ph-bpon \.ph-safe-t,#phone\.ph-bpon \.ph-safe-s\{display:none\}/);
});

test('Start in Big Picture mode is a switch on the phone too, and the start-up follows it', () => {
  assert.match(phone, /sw\('bpstart', !!S\.settings\.start_in_tv\)/);
  assert.match(phone, /if \(key === 'bpstart'\) return saveSettings\(\{ start_in_tv: on \}, true\);/);
  const starts = app.match(/^function startsInTV\(\) \{[^\n]*\}/m)[0];
  const run = (settings, pads = false, touch = true, handheld = false) => vm.runInNewContext(`${starts}; startsInTV()`, { S: { os: 'android', settings }, padHere: () => pads, touchHere: () => touch, droidHandheld: () => handheld });
  assert.equal(run({}), false);
  assert.equal(run({ start_in_tv: true }), true);
  assert.equal(run({}, true), true);
  assert.equal(run({}, false, false), true);
  // A handheld with a controller built in starts in Big Picture mode.
  assert.equal(run({}, false, true, true), true);
});

test('Optimization mode, Reduce motion and High contrast are saved and change the whole layout', () => {
  for (const k of ['lite', 'still', 'contrast']) assert.match(phone, new RegExp(`sw\\('${k}'`));
  assert.match(phone, /\{ lite: 'phone_lite', still: 'phone_still', contrast: 'phone_contrast' \}\[key\]/);
  assert.match(phone, /root\.classList\.toggle\('ph-lite', lite\(\)\);\n    root\.classList\.toggle\('ph-still', still\(\)\);\n    root\.classList\.toggle\('ph-hc', pref\.contrast\(\)\);/);
  // The phone's own Reduce motion setting counts too.
  assert.match(phone, /prefers-reduced-motion: reduce/);
  assert.match(css, /#phone\.ph-lite \.ph-ui \*\{box-shadow:none !important;text-shadow:none !important\}/);
  assert.match(css, /#phone\.ph-still \.ph-ui \*,/);
  assert.match(css, /#phone\.ph-hc\{/);
});

test('Trending is the top 10, filled from the most played when the week\'s chart is short', () => {
  assert.match(phone, /const TOP = 10;/);
  const ranked = phone.match(/^  function ranked\(\) \{[^]*?^  \}/m)[0];
  const list = (n) => Array.from({ length: n }, (_, i) => ({ id: 'g' + i, players: i }));
  const run = (chart) => vm.runInNewContext(`const TOP = 10; ${ranked}; ranked().map((a) => a.id)`, { pool: () => list(30), trendList: () => chart.map((a) => ({ a })), byDl: () => 0 });
  assert.equal(run(list(30)).length, 10);
  const short = run(list(3));
  assert.equal(short.length, 10);
  assert.deepEqual(short.slice(0, 3), ['g0', 'g1', 'g2']);
  assert.equal(new Set(short).size, 10);
});

test('All games and Big Picture\'s Catalog load as they scroll, not all at once', () => {
  assert.match(phone, /const shown = az\.slice\(0, P\.allN\);/);
  assert.match(phone, /if \(n\.id === 'ph-v-catalog'\) return moreGames\(n\);/);
  assert.match(phone, /const list = bpGrid\(\), shown = list\.slice\(0, bp\.n\);/);
  assert.match(phone, /if \(n\.classList\.contains\('ph-bpscroll'\)\) \{ if \(bp\) bpScrolled\(n\); return bpMore\(n\); \}/);
});

test('Search has one Consoles row instead of a wall of console chips', () => {
  const sr = phone.match(/^  function searchResults\(q\) \{[^]*?^  \}/m)[0];
  const empty = sr.slice(0, sr.indexOf('const hay'));
  assert.match(empty, /data-act="search-cons"/);
  assert.doesNotMatch(empty, /data-con=/);
});

test('the start-up screen\'s cover wall is not styled as its loading bar', () => {
  assert.match(phone, /<div class="ph-bootbar"><i><\/i><\/div>/);
  assert.doesNotMatch(css, /\.ph-boot \.ld\b/);
});

test('the game page\'s picture sits behind the page, with no masks to flash on older web engines', () => {
  assert.match(css, /\.ph-gph\{position:relative;z-index:0;/);
  assert.match(css, /\.ph-gpb\{position:relative;z-index:1;/);
  const gph = css.split('\n').filter((l) => l.startsWith('.ph-gph'));
  assert.ok(gph.length && gph.every((l) => !/mask/.test(l)));
});

test('the app icon is an adaptive icon whose mark fits every launcher shape, with a themed version', () => {
  for (const f of ['ic_launcher.xml', 'ic_launcher_round.xml']) {
    const x = read(`${RES}/mipmap-anydpi-v26/${f}`);
    assert.match(x, /<foreground android:drawable="@mipmap\/ic_launcher_foreground"\/>/);
    assert.match(x, /<monochrome android:drawable="@mipmap\/ic_launcher_monochrome"\/>/);
  }
  for (const d of ['mdpi', 'hdpi', 'xhdpi', 'xxhdpi', 'xxxhdpi']) {
    for (const f of ['ic_launcher_foreground', 'ic_launcher_monochrome', 'ic_launcher', 'ic_launcher_round']) assert.ok(exists(`${RES}/mipmap-${d}/${f}.png`), `${d}/${f}`);
  }
  // The old full-square drawing, which launchers cropped, is gone.
  assert.ok(!exists(`${RES}/drawable-v24/ic_launcher_foreground.xml`));
});

test('Android 12 and newer show a plain dark start, not the icon on a white screen', () => {
  for (const d of ['values-v31', 'values-night-v31']) {
    const x = read(`${RES}/${d}/themes.xml`);
    assert.match(x, /windowSplashScreenBackground">@color\/boot_background/);
    assert.match(x, /windowSplashScreenAnimatedIcon">@drawable\/splash_blank/);
  }
  assert.match(read(`${RES}/values/colors.xml`), /name="boot_background">#FF0B1220</);
});
