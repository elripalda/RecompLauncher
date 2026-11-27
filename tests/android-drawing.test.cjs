const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const read = (f) => fs.readFileSync(path.join(__dirname, '../ui', f), 'utf8');
const app = read('app.js'), phone = read('phone.js'), phoneCss = read('phone.css'), tvCss = read('tv.css'), html = read('index.html');
const fn = (src, name) => src.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];

test('Android never shows the desktop view, from the first frame', () => {
  // Marked from <head>, before the page's own markup is read.
  assert.ok(html.indexOf('<script src="droid.js"></script>') < html.indexOf('<body>'));
  const mark = (ua) => { const cls = new Set(); vm.runInNewContext(read('droid.js'), { navigator: { userAgent: ua }, document: { documentElement: { classList: { add: (c) => cls.add(c) } } } }); return cls.has('droid'); };
  assert.equal(mark('Mozilla/5.0 (Linux; Android 14; SM-S921B) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Mobile Safari/537.36'), true);
  assert.equal(mark('Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36 Edg/130.0.0.0'), false);
  assert.equal(mark('Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15'), false);
  assert.equal(mark('Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)'), false);
  assert.match(phoneCss, /html\.droid #app\{display:none\}/);
});

function renderContext(os) {
  const calls = [];
  const c = vm.createContext({
    S: { os, view: 'home', settings: { ui: {} } },
    expandLib: () => calls.push('expandLib'), paintTheme: () => calls.push('theme'), paintIsland: () => calls.push('island'),
    $: () => { calls.push('desktop'); throw new Error('desktop view drawn'); },
  });
  c.window = c;
  c.phoneRefresh = () => calls.push('phone');
  vm.runInContext(fn(app, 'render'), c);
  return { c, calls };
}
test('Android does not build the desktop view when something changes; the phone layout follows instead', () => {
  const { c, calls } = renderContext('android');
  c.render(true);
  assert.deepEqual(calls, ['expandLib', 'theme', 'island', 'phone']);
});
test('A PC still draws its desktop view', () => {
  const { c, calls } = renderContext('windows');
  assert.throws(() => c.render(true), /desktop view drawn/);
  assert.ok(!calls.includes('phone'));
});

test('Android draws lighter by default, a PC only when it is a handheld', () => {
  const run = (os, lighter, handheld) => {
    const on = {};
    const c = vm.createContext({ S: { os, handheld, settings: { lighter } }, document: { body: { classList: { toggle: (k, v) => { on[k] = v; } } } }, matchMedia: () => ({ matches: false }) });
    vm.runInContext(fn(app, 'applyLighter'), c);
    c.applyLighter();
    return on.lighter;
  };
  assert.equal(run('android', undefined, false), true);
  assert.equal(run('android', false, false), false);
  assert.equal(run('windows', undefined, false), false);
  assert.equal(run('linux', undefined, true), true);
});

test('The phone layout patches the page showing and keeps each page\'s place', () => {
  // The same page is patched (covers stay, nothing blinks); only another page is drawn fresh.
  assert.match(phone, /if \(was === key\) \{ morph\(root, markup\(\)\); return; \}/);
  assert.match(phone, /P\.tops\[was\] = old\.scrollTop/);
  // A game's size is text from the list ("24 MB"), shown as it is.
  assert.ok(!/fmtB\(a\.android_size\)/.test(phone));
});

test('A cover\'s height comes from its own width', () => {
  // Not from a flex column around it: older Android web engines draw no cover at all that way.
  assert.match(phoneCss, /\.ph-card\{display:block;/);
  assert.match(phoneCss, /\.ph-cover\{position:relative;display:block;width:100%;aspect-ratio:2\/3;/);
});

test('Every colour-mixing rule in Big Picture has a plain one for web engines that cannot mix', () => {
  const at = tvCss.indexOf('@supports not (color:color-mix(in srgb,red,blue)){');
  assert.ok(at > 0);
  const plain = tvCss.slice(at);
  assert.ok(!plain.slice(plain.indexOf('{')).includes('color-mix('), 'the plain rules mix nothing');
  const selectors = new Set();
  for (const line of tvCss.slice(0, at).split('\n')) {
    if (!line.includes('color-mix(')) continue;
    for (const m of line.matchAll(/(^|\})([^{}@]+)\{[^}]*color-mix\(/g)) selectors.add(m[2].trim());
  }
  assert.ok(selectors.size >= 12);
  for (const sel of selectors) assert.ok(plain.includes(`  ${sel}{`), `no plain rule for ${sel}`);
});
