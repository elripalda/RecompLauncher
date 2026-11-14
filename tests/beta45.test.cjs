const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const read = (f) => fs.readFileSync(path.join(__dirname, '../ui', f), 'utf8');
const app = read('app.js'), account = read('account.js');

// A function declaration out of a file, by name.
function fn(src, name) {
  const start = src.indexOf(`function ${name}(`);
  assert.ok(start >= 0, name);
  let i = src.indexOf('{', start), depth = 0;
  for (; i < src.length; i++) {
    if (src[i] === '{') depth++;
    if (src[i] === '}' && --depth === 0) break;
  }
  return src.slice(start, i + 1);
}

// A box with a size, a place on screen, how it overflows, and the scrolls asked of it.
function box(name, overflow, rect, inner, parent) {
  return { name, overflow, rect, inner, parentElement: parent, clientTop: 0, clientLeft: 0, clientHeight: rect.bottom - rect.top, clientWidth: rect.right - rect.left, scrollHeight: inner.h, scrollWidth: inner.w, asked: [], scrollBy(o) { this.asked.push(o); }, getBoundingClientRect() { return this.rect; } };
}
const reveal = new Function('getComputedStyle', `${fn(app, 'padReveal')} return padReveal;`)((el) => ({ overflowX: el.overflow || 'visible', overflowY: el.overflow || 'visible', scrollMarginTop: '20', scrollMarginBottom: '20', scrollMarginLeft: '0', scrollMarginRight: '0' }));

test('a controller move scrolls the page, never a box that only clips', () => {
  const body = { name: 'body' };
  const main = box('main', 'auto', { top: 0, bottom: 800, left: 0, right: 1000 }, { h: 3000, w: 1000 }, body);
  const banner = box('banner', 'hidden', { top: 900, bottom: 1400, left: 0, right: 1000 }, { h: 520, w: 1000 }, main);
  const button = { parentElement: banner, getBoundingClientRect: () => ({ top: 1300, bottom: 1340, left: 40, right: 200 }) };
  global.document = { body };
  reveal(button, 'auto');
  assert.deepEqual(banner.asked, []);
  assert.equal(main.asked.length, 1);
  assert.equal(main.asked[0].top, 1340 + 20 - 800);
});

test('a control already in view moves nothing', () => {
  const body = { name: 'body' };
  const main = box('main', 'auto', { top: 0, bottom: 800, left: 0, right: 1000 }, { h: 3000, w: 1000 }, body);
  const button = { parentElement: main, getBoundingClientRect: () => ({ top: 300, bottom: 340, left: 40, right: 200 }) };
  global.document = { body };
  reveal(button, 'auto');
  assert.deepEqual(main.asked, []);
});

test('a banner slide with four buttons of its own leaves Details to the cover', () => {
  const slideActs = (n) => new Function('actions', 'esc', 't', `${fn(app, 'slideActs')} return slideActs;`)(() => '<button>x</button>'.repeat(n), String, (s) => s)({ id: 'g' });
  assert.match(slideActs(3), /Details/);
  assert.doesNotMatch(slideActs(4), /Details/);
  assert.match(app, /class="sl-cover" data-act="open"/);
});

test('a save folder only noticed while playing says so', () => {
  const start = account.indexOf('const savesGuessed = ');
  const savesGuessed = new Function(`${account.slice(start, account.indexOf('\n', start))} return savesGuessed;`)();
  assert.equal(savesGuessed({ spots: [{ from: 'found' }] }), true);
  assert.equal(savesGuessed({ spots: [{ from: 'game' }] }), true);
  assert.equal(savesGuessed({ spots: [{ from: 'found' }, { from: 'developer' }] }), false);
  assert.equal(savesGuessed({ spots: [{ from: 'player' }] }), false);
  assert.equal(savesGuessed({ spots: [] }), false);
});

test('the Settings section list sits below the window bar', () => {
  const css = read('app.css');
  assert.match(css, /\.set-nav\{position:sticky;top:54px;/);
});

test('the monitor picker names monitors the same way on the desktop and in Big Picture', () => {
  const tv = read('tv.js');
  assert.match(app, /const monitorName = \(m\) => /);
  assert.ok((tv.match(/window\.monitorName\(/g) || []).length >= 3);
  assert.doesNotMatch(app + tv, /Windows Primary Display|Target Monitor/);
});
