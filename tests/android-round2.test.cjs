// The Android app after its second beta test: leaving and entering Big Picture without a glitch,
// the phone's Big Picture with every section and a full game page, reviews, and the settings.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const read = (f) => fs.readFileSync(path.join(__dirname, '..', f), 'utf8');
const phone = read('ui/phone.js'), css = read('ui/phone.css'), tv = read('ui/tv.js'), app = read('ui/app.js'), more = read('ui/more.js');
const slice = (s, a, b) => s.slice(s.indexOf(a), s.indexOf(b, s.indexOf(a)));

test('the screen turns behind a dark cover, and the page shows only once it has settled', () => {
  const enter = slice(phone, 'async function bpEnter(', 'async function bpLeave(');
  assert.match(enter, /box\.className = 'ph-bp cover';/);
  assert.match(enter, /await settle\(true, 220\);/);
  // Drawn only after the turn: nothing is ever drawn for the wrong shape.
  assert.ok(enter.indexOf("box.classList.remove('cover')") > enter.indexOf('await settle('));
  const leave = slice(phone, 'async function bpLeave(', '  // The last controller gone');
  assert.match(leave, /box\.classList\.add\('cover', 'out'\);/);
  assert.ok(leave.indexOf("android_orientation', { landscape: false }") > leave.indexOf("classList.add('cover', 'out')"));
  assert.ok(leave.indexOf('box.animate(') > leave.indexOf('await settle(null, 480)'));
  assert.match(css, /\.ph-bp\.cover \.ph-bpbg,\.ph-bp\.cover \.ph-bpin/);
  // While covered, nothing draws and the controller waits.
  assert.match(phone, /if \(!bp \|\| !el\.bpin \|\| bp\.busy\) return;/);
  assert.match(phone, /if \(bp\.busy\) return true;/);
});

test('the big-screen Big Picture on a tablet fades in and out over the phone layout too', () => {
  const enter = slice(tv, 'async function enter()', 'async function leave()');
  assert.ok(enter.indexOf('window.phoneLeave()') > enter.indexOf("root.classList.add('tv-cover')"));
  assert.match(enter, /window\.screenSettle\(true, 160\)/);
  const leave = slice(tv, 'async function leave()', 'window.enterTV = enter;');
  assert.match(leave, /root\.classList\.add\('tv-cover', 'out'\);\n      await wait\(still\(\) \? 0 : 220\);/);
  assert.match(leave, /window\.screenSettle\(null, 480\)/);
  assert.match(read('ui/tv.css'), /\.droid #tv\.tv-cover\{z-index:110\}/);
  // Android itself turns the window with a plain fade while the page has its cover up.
  assert.match(read('src-tauri/gen/android/app/src/main/java/com/recomplauncher/app/ScreenPlugin.kt'), /ROTATION_ANIMATION_CROSSFADE/);
});

test('the phone\'s Big Picture has every section of the big-screen one', () => {
  const tabs = phone.match(/const BP_TABS = \[(.*)\];/)[1];
  for (const k of ['home', 'library', 'catalog', 'downloads', 'ach', 'settings']) assert.match(tabs, new RegExp(`'${k}'`));
  const html = slice(phone, '  function bpHTML()', '  // The focused game\'s art fades in');
  for (const f of ['bpHome()', 'bpDownloads()', 'bpAch()', 'bpSettings()', 'bpGridHTML()', 'bpGameHTML(']) assert.ok(html.includes(f), f);
  // LB and RB go round all six.
  assert.match(phone, /bpTab\(\(bp\.tab \+ \(b === 'rb' \? 1 : BP_TABS\.length - 1\)\) % BP_TABS\.length\)/);
});

test('a game\'s page in Big Picture has reviews, what it is, its README, notes, updates and more like it', () => {
  const gp = slice(phone, '  function bpGameHTML(a)', '  function bpMenuHTML(');
  for (const f of ['revSection(a)', 'gameAbout(a)', 'pageRows(a)', 'bpAchGame(a)', 'moreLike(a']) assert.ok(gp.includes(f), f);
  const rows = slice(phone, '  function pageRows(a)', '  // About the game itself');
  for (const f of ['data-readme', 'data-act="notes"', 'data-act="rels"']) assert.ok(rows.includes(f), f);
  // The phone layout's own game page has them too.
  const page = slice(phone, '  function gameHTML(a)', '  // Where a rectangle sits');
  for (const f of ['gameAbout(a)', 'revSection(a)', 'pageRows(a)']) assert.ok(page.includes(f), f);
});

test('Home\'s shelf lines its covers up through their middle, with rows a press of down away', () => {
  assert.match(css, /\.ph-bpshelf\{[^}]*align-items:center/);
  assert.match(phone, /<div class="ph-bprows" data-key="rows">\$\{bpRows\(\)\}<\/div>/);
});

test('reviews: written in the phone\'s own sheet, read on the page, and asked about after playing on Android', () => {
  assert.match(more, /if \(window\.phoneOn && window\.phoneOn\(\) && window\.phoneReview\) return window\.phoneReview\(/);
  assert.match(more, /if \(window\.phoneOn && window\.phoneOn\(\) && window\.phoneSignInAsk\) return window\.phoneSignInAsk\(msg\);/);
  assert.match(phone, /window\.phoneReview = reviewSheet;/);
  // A problem needs 15 letters, as on the desktop and the website.
  assert.match(phone, /rvBad\(v\) \? Math\.max\(0, 15 - \[\.\.\.ta\.value\.trim\(\)\]\.length\) : 0/);
  // Android doesn't say when a game closes: coming back to the launcher after 10 minutes or more asks.
  assert.match(app, /if \(S\.os === 'android' && window\.droidPlayed\) window\.droidPlayed\(id\);/);
  assert.match(more, /if \(secs >= 600 && secs < 86400\) setTimeout\(\(\) => askRun\(p\.id, secs\), 1500\);/);
  // Reviews from Android say so, and the phone sends what it is instead of a PC's parts.
  assert.match(phone, /android: 'Android'/);
  assert.match(read('src-tauri/src/usage.rs'), /#\[cfg\(target_os = "android"\)\]\nfn read_pc\(\) -> PcInfo \{/);
});

test('Settings: Accessibility between Storage and About, Optimization mode, Refresh Catalog and Check for Game Updates', () => {
  const set = slice(phone, '  function viewSettings()', '  const VIEW = {');
  const at = (k) => set.indexOf(`data-key="${k}"`);
  assert.ok(at('stor') < at('ease') && at('ease') < at('about'));
  assert.match(set, /t\('Optimization mode'\)/);
  assert.doesNotMatch(phone, /t\('Performance mode'\)/);
  assert.match(set, /t\('Hide AI-assisted Recomps'\)/);
  assert.match(set, /data-act="refresh-cat"/);
  assert.match(set, /data-act="check-upd"/);
  assert.match(set, /sw\('revask'/);
});

test('each game shows once, as on the desktop, and its page lists its other versions', () => {
  assert.match(phone, /poolMemo = Object\.assign\(k, \{ list: oneEach\(visible\(\)\.filter\(\(a\) => forOS\(a\) && !a\.soon\)\) \}\);/);
  assert.match(phone, /\$\{verPillPh\(a\)\}/);
  assert.match(phone, /case 'vers': return versSheet\(id\);/);
  assert.match(slice(phone, '  function bpGameHTML(a)', '  function bpMenuHTML('), /data-act="vers"/);
});
