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

// Just enough of an element for menu(): attributes, a parent to be removed from, a size.
class El {
  constructor(cls) {
    this.className = cls || '';
    this.attrs = {};
    this.style = {};
    this.dataset = {};
    this.children = [];
    this.parent = null;
    this.offsetWidth = 200;
    this.offsetHeight = 300;
  }
  setAttribute(k, v) { this.attrs[k] = String(v); }
  getAttribute(k) { return k in this.attrs ? this.attrs[k] : null; }
  addEventListener() {}
  append(c) { c.parent = this; this.children.push(c); }
  remove() {
    if (!this.parent) return;
    this.parent.children = this.parent.children.filter((c) => c !== this);
    this.parent = null;
  }
  get isConnected() { return !!this.parent; }
  getBoundingClientRect() { return { left: 0, right: 100, top: 0, bottom: 40 }; }
}

// menu() and closeMenu() from app.js, with the page reduced to a body that holds menus.
function page() {
  const body = new El('body');
  const c = vm.createContext({
    Element: El,
    MutationObserver: class { observe() {} },
    document: { body, createElement: () => new El() },
    $: () => ({ focus() {} }),
    $$: () => body.children.filter((m) => m.className === 'menu'),
    matchMedia: () => ({ matches: true }),
    menuRows: () => '',
    esc: (s) => s,
    innerWidth: 1200,
    innerHeight: 800,
  });
  vm.runInContext(`${fn('closeMenu')}\n${fn('menu')}`, c);
  // The page's click handler closes any open menu before the clicked button acts.
  const click = (btn) => { c.closeMenu(); c.menu(btn, [{ act: 'pin', label: 'Pin' }]); };
  return { body, click };
}

test('a click on a "…" button opens its menu', () => {
  const { body, click } = page();
  const more = new El();
  click(more);
  assert.equal(body.children.length, 1);
  assert.equal(more.getAttribute('aria-expanded'), 'true');
});

test('a second click on the same "…" button closes its menu', () => {
  const { body, click } = page();
  const more = new El();
  click(more);
  click(more);
  assert.equal(body.children.length, 0);
  assert.equal(more.getAttribute('aria-expanded'), 'false');
});

test('a third click on the same "…" button opens its menu again', () => {
  const { body, click } = page();
  const more = new El();
  click(more);
  click(more);
  click(more);
  assert.equal(body.children.length, 1);
});

test('a click on another game\'s "…" button moves the menu there', () => {
  const { body, click } = page();
  const one = new El(), two = new El();
  click(one);
  click(two);
  assert.equal(body.children.length, 1);
  assert.equal(two.getAttribute('aria-expanded'), 'true');
});
