'use strict';
// The quick menu over a running game. The core shows this window (Ctrl+F12, or holding View on a
// controller), sends `overlay-open` with the game, and `pad` for the controller.
const T = window.__TAURI__;
const invoke = (c, a) => T.core.invoke(c, a);
const stage = document.getElementById('stage');
const items = [...document.querySelectorAll('.items button')];
let at = 0;

function fit() { stage.style.transform = `scale(${innerHeight / 1080})`; }
addEventListener('resize', fit);
fit();

function mark() { items.forEach((b, i) => b.classList.toggle('f', i === at)); items[at].focus({ preventScroll: true }); }
function mins(s) { const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60); return h ? tf('{h} h {m} min', { h, m }) : tf('{m} min', { m: Math.max(1, m) }); }
// The player's language, from the launcher's settings: read each time the menu opens, in case it changed.
async function lang() {
  try { const st = await invoke('get_state'); const l = ((st.settings || {}).ui || {}).lang; const want = l && l !== 'auto' ? l : systemLang(); if (want !== curLang() || !lang.done) { await loadLang(want); tDom(); lang.done = true; } } catch (e) { /* stays in English */ }
}
lang();
function act(a) {
  stage.classList.remove('in');
  invoke('overlay_action', { action: a });
}
function press(n) {
  if (n === 'up') at = (at + items.length - 1) % items.length;
  else if (n === 'down') at = (at + 1) % items.length;
  else if (n === 'a') return act(items[at].dataset.a);
  else if (n === 'b' || n === 'menu' || n === 'view') return act('resume');
  mark();
}

T.event.listen('overlay-open', async ({ payload: p }) => {
  await lang();
  document.getElementById('name').textContent = p.name;
  document.getElementById('sub').textContent = tf('Playing for {time} · {total} total', { time: mins(p.session), total: mins(p.played + p.session) });
  document.getElementById('shots').textContent = p.shots ? String(p.shots) : '';
  document.getElementById('clock').textContent = new Date().toLocaleTimeString(curLocale(), { hour: 'numeric', minute: '2-digit' });
  at = 0;
  mark();
  fit();
  requestAnimationFrame(() => stage.classList.add('in'));
});
// Controller presses meant for this menu only, and only while it's showing: the launcher's own
// window gets presses too, and a hidden menu must never act on them (a press of A in Big Picture
// mode would otherwise pick "Screenshots and library" here).
const ownWin = T.webviewWindow && T.webviewWindow.getCurrentWebviewWindow && T.webviewWindow.getCurrentWebviewWindow();
const ownListen = ownWin && typeof ownWin.listen === 'function' ? ownWin.listen.bind(ownWin) : T.event.listen;
ownListen('pad', ({ payload }) => { if (stage.classList.contains('in')) press(typeof payload === 'string' ? payload : payload.b); });
document.addEventListener('keydown', (e) => {
  const m = { ArrowUp: 'up', ArrowDown: 'down', Enter: 'a', ' ': 'a', Escape: 'b' }[e.key];
  if (m) { e.preventDefault(); press(m); }
});
items.forEach((b, i) => {
  b.addEventListener('mouseenter', () => { at = i; mark(); });
  b.addEventListener('click', () => act(b.dataset.a));
});
// Clicking beside the menu goes back to the game.
stage.addEventListener('click', (e) => { if (!e.target.closest('.panel')) act('resume'); });
