// Test control's helper inside the launcher's window (see mod.rs). The launcher puts it in the
// page only when test control is on, so players never have it. Every call from the MCP server
// runs `window.__rlTest.run(id, op, args, fn)`, and the answer goes back on the rl-test-reply event.
(() => {
  const VERSION = 1;
  if (window.__rlTest && window.__rlTest.v === VERSION) return;
  const T = window.__TAURI__;
  const REPLY = 'rl-test-reply';
  const LOG_MAX = 300;
  const consoleLog = [];
  const events = [];
  let eventSeq = 0;
  let refSeq = 0;

  const clip = (s, n) => { const t = String(s == null ? '' : s); return t.length > n ? t.slice(0, n) + '…' : t; };
  const squash = (s) => String(s || '').replace(/\s+/g, ' ').trim();
  const push = (list, item) => { list.push(item); if (list.length > LOG_MAX) list.shift(); };
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));

  // What the page printed: errors and warnings are what a tester needs, so those are kept.
  for (const level of ['error', 'warn']) {
    const orig = console[level].bind(console);
    console[level] = (...a) => {
      push(consoleLog, { t: Date.now(), level, text: clip(a.map((x) => (x instanceof Error ? x.stack || x.message : typeof x === 'string' ? x : safeJson(x))).join(' '), 2000) });
      orig(...a);
    };
  }
  // ui_eval calls whose code hasn't started yet. Code that doesn't parse never starts, and the
  // browser reports it here instead: that answers the call at once, rather than leaving it to wait.
  const evalsDue = [];
  addEventListener('error', (e) => {
    push(consoleLog, { t: Date.now(), level: 'uncaught', text: clip(`${e.message} (${e.filename || ''}:${e.lineno || 0})`, 2000) });
    if (evalsDue.length && /SyntaxError/.test(String(e.message))) send(evalsDue.pop(), false, `The code doesn't parse: ${e.message}`);
  });
  addEventListener('unhandledrejection', (e) => push(consoleLog, { t: Date.now(), level: 'unhandled-rejection', text: clip(e.reason && (e.reason.stack || e.reason.message) || e.reason, 2000) }));

  function safeJson(x) {
    try { return JSON.stringify(x); } catch (e) { return String(x); }
  }

  // The launcher's own events that the window listens to, so a test can follow a download or a
  // game starting without watching the screen.
  const WATCHED = ['toast', 'install', 'game', 'setup', 'note', 'goto', 'open-game', 'net', 'convert', 'move', 'unpacked', 'first-setup', 'bigpicture', 'catalog-fresh', 'launcher-update'];
  for (const name of WATCHED) {
    try {
      T.event.listen(name, (ev) => push(events, { seq: ++eventSeq, t: Date.now(), name, payload: clip(safeJson(ev.payload), 1500) }));
    } catch (e) { /* that event isn't there in this build */ }
  }

  // A small mark on the window, so nobody forgets the launcher is being driven.
  function badge() {
    if (document.getElementById('rl-test-badge')) return;
    const b = document.createElement('div');
    b.id = 'rl-test-badge';
    b.textContent = 'Test control on';
    b.style.cssText = 'position:fixed;right:8px;bottom:8px;z-index:2147483647;pointer-events:none;font:600 11px/1 system-ui,sans-serif;color:#0b1220;background:#fbbf24;border-radius:6px;padding:4px 7px;opacity:.85';
    document.body.appendChild(b);
  }

  const visible = (el) => {
    if (!el || !el.isConnected) return false;
    if (el.closest('[hidden],[aria-hidden="true"],[inert]')) return false;
    const r = el.getBoundingClientRect();
    if (r.width < 1 || r.height < 1) return false;
    const cs = getComputedStyle(el);
    return cs.visibility !== 'hidden' && cs.display !== 'none' && Number(cs.opacity) > 0.01;
  };
  const inView = (el) => { const r = el.getBoundingClientRect(); return r.bottom > 0 && r.right > 0 && r.top < innerHeight && r.left < innerWidth; };
  const tvOn = () => document.body.classList.contains('tv-on');

  // What's in front: the top open dialog, else Big Picture, else the whole window.
  function scope() {
    const dialogs = [...document.querySelectorAll('[role="dialog"],[role="alertdialog"]')].filter(visible);
    if (dialogs.length) return dialogs[dialogs.length - 1];
    if (tvOn()) return document.getElementById('tv') || document.body;
    return document.body;
  }

  // Big Picture's tiles and rows are plain elements with a data-key, moved between with the controller.
  const CLICKABLE = 'button,a[href],input,select,textarea,summary,[role="button"],[role="link"],[role="tab"],[role="switch"],[role="checkbox"],[role="radio"],[role="menuitem"],[role="menuitemradio"],[role="option"],[data-act],[data-go],[data-key],[tabindex]:not([tabindex="-1"])';

  function labelOf(el) {
    const aria = el.getAttribute('aria-label');
    if (aria) return squash(aria);
    const by = el.getAttribute('aria-labelledby');
    if (by) { const l = by.split(/\s+/).map((i) => document.getElementById(i)).filter(Boolean).map((x) => x.innerText).join(' '); if (squash(l)) return squash(l); }
    if (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA') return squash(el.placeholder || el.name || el.id || el.type);
    const text = squash(el.innerText);
    if (text) return text;
    const img = el.querySelector('img[alt]');
    return squash(el.title || (img && img.alt) || '');
  }

  function describe(el) {
    if (!el.dataset.rlt) el.dataset.rlt = 'e' + (++refSeq);
    const role = el.getAttribute('role') || el.tagName.toLowerCase();
    const d = { ref: el.dataset.rlt, role, label: clip(labelOf(el), 120) };
    if (el.disabled || el.getAttribute('aria-disabled') === 'true') d.disabled = true;
    const checked = el.getAttribute('aria-checked') || el.getAttribute('aria-selected') || el.getAttribute('aria-pressed');
    if (checked) d.checked = checked;
    if (el.type === 'checkbox' || el.type === 'radio') d.checked = String(el.checked);
    if ('value' in el && el.tagName !== 'BUTTON' && el.value !== undefined) d.value = clip(el.value, 200);
    if (el.tagName === 'SELECT') d.options = [...el.options].slice(0, 40).map((o) => ({ value: o.value, label: clip(squash(o.textContent), 60) }));
    if (el.dataset.act) d.act = el.dataset.act;
    if (el.dataset.go) d.go = el.dataset.go;
    if (el.dataset.id) d.id = el.dataset.id;
    if (document.activeElement === el) d.focused = true;
    if (el.dataset.key) d.key = el.dataset.key;
    // The controller's highlight: pad-focus in the desktop window, f in Big Picture.
    if (el.classList.contains('pad-focus') || (tvOn() && el.classList.contains('f'))) d.highlighted = true;
    if (!inView(el)) d.offscreen = true;
    return d;
  }

  function page() {
    const S0 = typeof S === 'object' ? S : {};
    const sc = scope();
    const dialog = sc !== document.body && sc.getAttribute('role') ? clip(sc.getAttribute('aria-label') || squash((sc.querySelector('h1,h2,h3') || {}).innerText), 120) : null;
    const out = { view: S0.view, arg: S0.arg == null ? null : S0.arg, tab: S0.view === 'game' ? S0.tab : S0.view === 'settings' ? S0.setTab : null, bigPicture: tvOn(), dialog, offline: !!S0.offline, title: document.title };
    // In Big Picture mode its own page is the one in front.
    if (out.bigPicture && typeof window.tvWhere === 'function') Object.assign(out, window.tvWhere(), { tab: null });
    if (out.bigPicture) { const f = document.querySelector('#tv .f'); out.highlighted = f ? clip(labelOf(f), 120) : null; }
    return out;
  }

  function snapshot(a) {
    const max = Math.min(Math.max(Number(a.max) || 150, 1), 600);
    const sc = scope();
    const all = [...sc.querySelectorAll(CLICKABLE)].filter((el) => el.id !== 'rl-test-badge' && visible(el));
    // On screen first: those are what a person would see and press.
    const ordered = all.filter(inView).concat(all.filter((el) => !inView(el)));
    const elements = ordered.slice(0, max).map(describe);
    const headings = [...sc.querySelectorAll('h1,h2,h3')].filter(visible).slice(0, 30).map((h) => clip(squash(h.innerText), 120)).filter(Boolean);
    const toasts = [...document.querySelectorAll('.toast,[role="status"],[role="alert"]')].filter(visible).map((x) => clip(squash(x.innerText), 300)).filter(Boolean);
    const out = { page: page(), headings, toasts, elements, more: Math.max(all.length - elements.length, 0) };
    if (a.text) out.text = clip(sc.innerText, Math.min(Number(a.text_chars) || 6000, 60000));
    return out;
  }

  // The element a call names: a ref from ui_snapshot, a CSS selector, or the words on it.
  function find(a) {
    if (a.ref) {
      const el = document.querySelector(`[data-rlt="${CSS.escape(String(a.ref))}"]`);
      if (!el) throw new Error(`Nothing has ref ${a.ref} now (the page was drawn again). Take a new ui_snapshot.`);
      return el;
    }
    if (a.selector) {
      const list = [...document.querySelectorAll(String(a.selector))].filter(visible);
      const el = list[Number(a.index) || 0];
      if (!el) throw new Error(`No visible element matches ${a.selector}${a.index ? ` at index ${a.index}` : ''}.`);
      return el;
    }
    if (a.text) {
      const want = squash(a.text).toLowerCase();
      const list = [...scope().querySelectorAll(CLICKABLE)].filter(visible);
      const exact = list.filter((el) => labelOf(el).toLowerCase() === want);
      const near = exact.length ? exact : list.filter((el) => labelOf(el).toLowerCase().includes(want));
      const el = near[Number(a.index) || 0];
      if (!el) throw new Error(`Nothing to press says "${a.text}" in what's in front (${page().dialog ? 'a dialog' : page().bigPicture ? 'Big Picture' : 'the page'}).`);
      return el;
    }
    throw new Error('Name the element with ref, selector or text.');
  }

  async function click(a) {
    const el = find(a);
    if (el.disabled || el.getAttribute('aria-disabled') === 'true') throw new Error(`"${labelOf(el)}" is disabled.`);
    // Described before the click: pressing it can change what it says ("Play" turns into
    // "Back to the Game").
    const clicked = describe(el);
    el.scrollIntoView({ block: 'center', inline: 'nearest' });
    if (typeof el.focus === 'function') el.focus({ preventScroll: true });
    el.click();
    await wait(Number(a.settle_ms) >= 0 ? Number(a.settle_ms) : 350);
    return { clicked, page: page() };
  }

  async function type(a) {
    const el = find(a);
    if (!('value' in el)) throw new Error(`"${labelOf(el)}" isn't a box you can type in.`);
    el.focus({ preventScroll: false });
    el.value = a.clear === false ? el.value + String(a.value) : String(a.value);
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
    if (a.submit) await key({ key: 'Enter' }, el);
    await wait(200);
    return { typed: describe(el), page: page() };
  }

  const CODES = { Enter: 'Enter', Escape: 'Escape', Tab: 'Tab', ' ': 'Space', ArrowUp: 'ArrowUp', ArrowDown: 'ArrowDown', ArrowLeft: 'ArrowLeft', ArrowRight: 'ArrowRight', Backspace: 'Backspace' };
  async function key(a, target) {
    const k = String(a.key || '');
    if (!k) throw new Error('Name the key, like Enter, Escape, ArrowDown or a letter.');
    const el = target || (a.ref || a.selector ? find(a) : document.activeElement || document.body);
    const init = { key: k, code: CODES[k] || (k.length === 1 ? 'Key' + k.toUpperCase() : k), bubbles: true, cancelable: true, ctrlKey: !!a.ctrl, shiftKey: !!a.shift, altKey: !!a.alt, metaKey: !!a.meta };
    const down = el.dispatchEvent(new KeyboardEvent('keydown', init));
    // A plain Enter or Space on a button presses it, as the browser itself would.
    const presses = down && (k === 'Enter' || k === ' ') && (el.tagName === 'BUTTON' || el.getAttribute('role') === 'button');
    if (presses) el.click();
    el.dispatchEvent(new KeyboardEvent('keyup', init));
    await wait(Number(a.settle_ms) >= 0 ? Number(a.settle_ms) : 250);
    return { pressed: k, page: page() };
  }

  async function navigate(a) {
    const to = String(a.page || '');
    if (to === 'back') { back(); await wait(400); return { page: page() }; }
    if (to === 'bigpicture') {
      if (typeof window.startTV !== 'function') throw new Error('Big Picture mode is not in this build.');
      if (!tvOn()) window.startTV();
      // Its start animation takes the controller only once something is highlighted.
      const until = Date.now() + 10000;
      const ready = () => document.querySelector('#tv .f') && !(window.tvWhere && window.tvWhere().starting);
      while (!ready() && Date.now() < until) await wait(150);
      // The start-up's last fade.
      await wait(400);
      return { page: page() };
    }
    if (to === 'desktop') {
      if (tvOn()) await window.leaveTV();
      await wait(600);
      return { page: page() };
    }
    const PAGES = ['home', 'discover', 'library', 'achievements', 'game', 'downloads', 'settings', 'shots', 'soon', 'saves'];
    if (!PAGES.includes(to) && !to.startsWith('plug:')) throw new Error(`No page called ${to}. Pages: ${PAGES.join(', ')}, back, bigpicture, desktop, or plug:<plugin id>.`);
    if (tvOn()) throw new Error('Big Picture mode is open: move around it with controller_press, or leave it with page desktop.');
    if (to === 'game' && !a.id) throw new Error('A game page needs the game\'s id (see launcher_status).');
    if (to === 'game' && a.tab) S.nextTab = String(a.tab);
    if (to === 'settings' && a.tab) S.setTab = String(a.tab);
    go(to, to === 'game' ? String(a.id) : a.id == null ? null : String(a.id));
    await wait(500);
    return { page: page() };
  }

  async function waitFor(a) {
    const until = Date.now() + Math.min(Math.max(Number(a.timeout_ms) || 10000, 100), 120000);
    const text = a.text ? squash(a.text).toLowerCase() : '';
    const met = () => {
      let found = true;
      if (a.selector) found = [...document.querySelectorAll(String(a.selector))].some(visible);
      if (found && text) found = squash(document.body.innerText).toLowerCase().includes(text);
      if (found && a.page) found = page().view === a.page;
      return a.gone ? !found : found;
    };
    if (!a.selector && !text && !a.page) throw new Error('Wait for text, a selector or a page.');
    for (;;) {
      if (met()) return { met: true, page: page() };
      if (Date.now() > until) return { met: false, page: page() };
      await wait(150);
    }
  }

  async function command(a) {
    const name = String(a.name || '');
    if (!name) throw new Error('Name the command.');
    const call = typeof invoke === 'function' ? invoke : T.core.invoke;
    const started = call(name, a.args || {});
    if (a.wait === false) { started.catch(() => {}); return { started: true }; }
    return { result: await started };
  }

  function logs(a) {
    const n = Math.min(Math.max(Number(a.lines) || 200, 1), LOG_MAX);
    return { console: consoleLog.slice(-n) };
  }

  function eventsSince(a) {
    const since = Number(a.since) || 0;
    return { events: events.filter((e) => e.seq > since), last: eventSeq };
  }

  function send(id, ok, value) {
    let v = value;
    try { v = JSON.parse(JSON.stringify(value === undefined ? null : value)); } catch (e) { v = String(value); }
    T.event.emit(REPLY, { id, ok, value: v }).catch(() => {});
  }

  function expectEval(id) {
    evalsDue.push(id);
  }

  async function run(id, op, args, fn) {
    const reply = (ok, value) => send(id, ok, value);
    const due = evalsDue.indexOf(id);
    if (due >= 0) evalsDue.splice(due, 1);
    try {
      badge();
      const a = args || {};
      const ops = { snapshot, click, type, key: (x) => key(x), navigate, wait: waitFor, command, logs, events: eventsSince, ping: () => ({ page: page() }) };
      if (op === 'eval') return reply(true, await fn(a));
      if (!ops[op]) throw new Error(`Unknown step ${op}.`);
      reply(true, await ops[op](a));
    } catch (e) {
      reply(false, e && e.message ? e.message : String(e));
    }
  }

  window.__rlTest = { v: VERSION, run, expectEval };
  if (document.body) badge(); else addEventListener('DOMContentLoaded', badge);
})();
