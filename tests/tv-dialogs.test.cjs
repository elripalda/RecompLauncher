const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const read = (f) => fs.readFileSync(path.join(__dirname, '../ui', f), 'utf8');
const tvCss = read('tv.css'), tvJs = read('tv.js'), appCss = read('app.css'), appJs = read('app.js');

test('the island that holds the messages is centred on the whole window, in TV mode too', () => {
  assert.match(appCss, /\.island\{position:fixed;left:50%;/);
});

test('TV mode does not shift the messages again inside the centred island', () => {
  assert.doesNotMatch(tvCss, /body\.tv-on \.toasts\{[^}]*transform/);
});

test('What\'s New gets a TV-sized window in TV mode', () => {
  assert.match(tvCss, /body\.tv-on \.modal:has\(\.whatsnew\)\{width:min\(1040px,88vw\)\}/);
});

test('the way out on Home offers Quit RecompLauncher', () => {
  assert.match(tvJs, /label: t\('Quit RecompLauncher'\), icon: 'close', run: \(\) => invoke\('quit_launcher'\)/);
});

test('the quit command is registered with the window', () => {
  const lib = fs.readFileSync(path.join(__dirname, '../src-tauri/src/lib.rs'), 'utf8');
  assert.match(lib, /tray::quit_launcher/);
});

// ---------- the controller on a desktop dialog over Big Picture, run for real ----------
// The desktop's controller handling (window.deskPad, from app.js) and Big Picture's press() with the
// dialog code before it (from tv.js), run together on a small fake page. press() is cut out of
// tv.js's wrapper as it is: with a dialog open it returns before touching anything else of TV mode.
const deskStart = appJs.indexOf('// ---------- a controller on the desktop view ----------');
const deskEnd = appJs.indexOf('// The window has no system title bar', deskStart);
const tvStart = tvJs.indexOf('  // The notes scroll inside the dialog');
const pressStart = tvJs.indexOf('  function press(name) {', tvStart);
const tvEnd = tvJs.indexOf('\n  }\n', pressStart) + 5;
assert.ok(deskStart >= 0 && deskEnd > deskStart);
assert.ok(tvStart >= 0 && pressStart > tvStart && tvEnd > pressStart);

// Just enough of a CSS selector for the ones this code asks about: a tag, #id, .class, [attr],
// [attr="value"] and :not([attr...]), joined into lists with commas.
function matchOne(el, sel) {
  let rest = sel.trim();
  while (rest) {
    let m;
    if ((m = rest.match(/^[a-z]+/i))) {
      if (el.tagName !== m[0].toUpperCase()) return false;
    } else if ((m = rest.match(/^#([\w-]+)/))) {
      if (el.id !== m[1]) return false;
    } else if ((m = rest.match(/^\.([\w-]+)/))) {
      if (!el.classList.contains(m[1])) return false;
    } else if ((m = rest.match(/^:not\(([^)]*)\)/))) {
      if (matchOne(el, m[1])) return false;
    } else if ((m = rest.match(/^\[([\w-]+)(?:="([^"]*)")?\]/))) {
      const value = el.getAttribute(m[1]);
      if (value === null) return false;
      if (m[2] !== undefined && value !== m[2]) return false;
    } else {
      throw new Error(`fake page can't read selector ${sel}`);
    }
    rest = rest.slice(m[0].length);
  }
  return true;
}
const matches = (el, list) => list.split(',').some((sel) => matchOne(el, sel));

function page() {
  const document = { activeElement: null, clicks: [] };
  const make = (tag, { cls = '', attrs = {}, rect = { left: 0, top: 0, right: 0, bottom: 0 }, kids = [] } = {}) => {
    const names = new Set(cls.split(' ').filter(Boolean));
    const el = {
      tagName: tag.toUpperCase(), id: attrs.id || '', dataset: {}, children: [], parentElement: null, scrolls: [],
      scrollTop: 0, clientHeight: rect.bottom - rect.top, scrollHeight: rect.bottom - rect.top,
      classList: { add: (x) => names.add(x), remove: (x) => names.delete(x), contains: (x) => names.has(x) },
      getAttribute: (name) => {
        if (name === 'class') return [...names].join(' ');
        if (name.startsWith('data-')) return el.dataset[name.slice(5)] ?? null;
        return name in attrs ? attrs[name] : null;
      },
      getBoundingClientRect: () => ({ ...rect, width: rect.right - rect.left, height: rect.bottom - rect.top }),
      matches: (sel) => matches(el, sel),
      closest: (sel) => {
        for (let at = el; at; at = at.parentElement) if (at.matches(sel)) return at;
        return null;
      },
      contains: (other) => {
        for (let at = other; at; at = at.parentElement) if (at === el) return true;
        return false;
      },
      querySelectorAll: (sel) => el.children.flatMap((kid) => [...(kid.matches(sel) ? [kid] : []), ...kid.querySelectorAll(sel)]),
      querySelector: (sel) => el.querySelectorAll(sel)[0] || null,
      append: (kid) => { kid.parentElement = el; el.children.push(kid); },
      remove: () => {
        const up = el.parentElement;
        if (!up) return;
        up.children.splice(up.children.indexOf(el), 1);
        el.parentElement = null;
      },
      focus: () => { document.activeElement = el; },
      click: () => { document.clicks.push(el); },
      scrollBy: (to) => { el.scrolls.push(to.top); },
      get isConnected() { return document.body.contains(el); },
    };
    kids.forEach((kid) => el.append(kid));
    return el;
  };
  document.body = make('body');
  document.activeElement = document.body;
  document.querySelectorAll = (sel) => document.body.querySelectorAll(sel);
  document.querySelector = (sel) => document.body.querySelector(sel);
  document.addEventListener = () => {};
  return { document, make };
}

// A dialog like Restart Steam: some text and two buttons side by side, Not now and the main one.
function dialog(make, { overflow = false } = {}) {
  const later = make('button', { cls: 'btn', rect: { left: 100, top: 200, right: 200, bottom: 240 } });
  const main = make('button', { cls: 'btn main', rect: { left: 220, top: 200, right: 320, bottom: 240 } });
  const text = make('p', { rect: { left: 20, top: 20, right: 380, bottom: 180 } });
  const box = make('div', { cls: 'modal', rect: { left: 0, top: 0, right: 400, bottom: 300 }, kids: [text, later, main] });
  // Text that goes on past the bottom edge, as What's New does.
  if (overflow) box.scrollHeight = 1000;
  const scrim = make('div', { cls: 'scrim', kids: [box] });
  return { scrim, box, main, later };
}

function bigPicture({ desktop = true } = {}) {
  const { document, make } = page();
  // The desktop's controller code listens on the window too (it lets go of the highlight when the
  // window loses focus); nothing here needs those listeners to fire.
  const window = { addEventListener: () => {} };
  const ctx = vm.createContext({
    window, document, SU: {}, S: { back: [], view: 'home' }, TV: { on: true }, root: null,
    $: (s, r = document) => r.querySelector(s), $$: (s, r = document) => [...r.querySelectorAll(s)],
    MutationObserver: class { observe() {} },
    getComputedStyle: () => ({ visibility: 'visible', overflowX: 'visible', overflowY: 'visible' }),
    performance: { now: () => 1000 }, calm: () => true, setTimeout: () => {}, requestAnimationFrame: () => {},
  });
  if (desktop) vm.runInContext(appJs.slice(deskStart, deskEnd), ctx);
  vm.runInContext(tvJs.slice(tvStart, tvEnd), ctx);
  const open = (options) => {
    const d = dialog(make, options);
    document.body.append(d.scrim);
    return d;
  };
  return { document, open, press: (name) => ctx.press(name) };
}

test('A on a dialog with nothing lit presses its main button at once', () => {
  const tv = bigPicture();
  const d = tv.open();
  tv.press('a');
  assert.deepEqual(tv.document.clicks, [d.main]);
});

test('the first d-pad press on a dialog lights its main button without pressing it', () => {
  const tv = bigPicture();
  const d = tv.open();
  tv.press('down');
  assert.ok(d.main.classList.contains('pad-focus'));
  assert.deepEqual(tv.document.clicks, []);
});

test('the d-pad moves the highlight to a dialog\'s other button, and A presses that one', () => {
  const tv = bigPicture();
  const d = tv.open();
  tv.press('down');
  tv.press('left');
  assert.ok(d.later.classList.contains('pad-focus'));
  assert.ok(!d.main.classList.contains('pad-focus'));
  tv.press('a');
  assert.deepEqual(tv.document.clicks, [d.later]);
});

test('B closes a dialog', () => {
  const tv = bigPicture();
  const d = tv.open();
  tv.press('b');
  assert.equal(d.scrim.isConnected, false);
});

test('B leaves a busy dialog open', () => {
  const tv = bigPicture();
  const d = tv.open();
  d.scrim.dataset.busy = '1';
  tv.press('b');
  assert.equal(d.scrim.isConnected, true);
});

test('down scrolls a dialog\'s long text while no choice shows below the lit one', () => {
  const tv = bigPicture();
  const d = tv.open({ overflow: true });
  tv.press('down');
  tv.press('down');
  assert.deepEqual(d.box.scrolls, [200]);
  assert.ok(d.main.classList.contains('pad-focus'));
});

test('the controller answers the newest dialog when one opened another', () => {
  const tv = bigPicture();
  const first = tv.open();
  const second = tv.open();
  tv.press('a');
  assert.deepEqual(tv.document.clicks, [second.main]);
  tv.press('b');
  assert.equal(second.scrim.isConnected, false);
  assert.equal(first.scrim.isConnected, true);
});

test('without the desktop\'s controller handling, up and down still scroll a dialog', () => {
  const tv = bigPicture({ desktop: false });
  const d = tv.open();
  tv.press('down');
  tv.press('up');
  assert.deepEqual(d.box.scrolls, [200, -200]);
});
