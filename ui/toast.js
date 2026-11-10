'use strict';
// Achievement toasts over the game, top right. The core queues them (ach.rs) and pokes this window
// with `ach-toast`; they're shown one at a time, about four seconds each, never stacked.
const T = window.__TAURI__;
const invoke = (c, a) => T.core.invoke(c, a);
const el = document.getElementById('toast');
const queue = [];
let busy = false;

async function lang() {
  try { const st = await invoke('get_state'); const l = ((st.settings || {}).ui || {}).lang; const want = l && l !== 'auto' ? l : systemLang(); if (want !== curLang()) await loadLang(want); } catch (e) { /* stays in English */ }
}
const ready = lang();

// A short two-note chime, made here (no sound file to ship).
function chime(big) {
  try {
    const ac = new (window.AudioContext || window.webkitAudioContext)();
    const notes = big ? [784, 988, 1319] : [880, 1319];
    notes.forEach((f, i) => {
      const o = ac.createOscillator(), g = ac.createGain();
      o.type = 'sine';
      o.frequency.value = f;
      const t0 = ac.currentTime + i * 0.11;
      g.gain.setValueAtTime(0, t0);
      g.gain.linearRampToValueAtTime(0.16, t0 + 0.02);
      g.gain.exponentialRampToValueAtTime(0.001, t0 + 0.5);
      o.connect(g).connect(ac.destination);
      o.start(t0);
      o.stop(t0 + 0.55);
    });
    setTimeout(() => ac.close(), 1500);
  } catch (e) { /* no sound */ }
}

const wait = (ms) => new Promise((r) => setTimeout(r, ms));
// Leaderboard values read the same as on the site: score and value as plain numbers; frames and time
// in 1/60 s as m:ss.cc; millisecs m:ss.mmm; seconds m:ss; minutes h:mm. Hours go in front when there are any.
function fmtValue(v, format) {
  v = Math.trunc(Number(v) || 0);
  const neg = v < 0 ? '-' : '', a = Math.abs(v), p2 = (n) => String(n).padStart(2, '0');
  const clock = (secs, tail) => { const h = Math.floor(secs / 3600), m = Math.floor(secs / 60) % 60, s = secs % 60; return h > 0 ? `${neg}${h}:${p2(m)}:${p2(s)}${tail}` : `${neg}${m}:${p2(s)}${tail}`; };
  switch (format) {
    case 'frames': case 'time': return clock(Math.floor(a / 60), '.' + p2(Math.floor((a % 60) * 100 / 60)));
    case 'millisecs': return clock(Math.floor(a / 1000), '.' + String(a % 1000).padStart(3, '0'));
    case 'seconds': return clock(a, '');
    case 'minutes': return `${neg}${Math.floor(a / 60)}:${p2(a % 60)}`;
    default: return String(v);
  }
}
const TIMED = ['frames', 'time', 'millisecs', 'seconds', 'minutes'];

async function show(c) {
  // Mastered and Beaten are the big cards (Beaten in silver); events and new bests wear the gold star.
  const big = !!(c.mastered || c.beaten);
  el.classList.remove('in', 'out', 'big', 'silver', 'gold');
  el.classList.toggle('big', big);
  el.classList.toggle('silver', !!c.beaten && !c.mastered);
  el.classList.toggle('gold', !big && !!(c.event || c.best));
  const kicker = c.mastered ? t('Every Achievement') : c.beaten ? t('Game Beaten') : c.event ? t('Event Complete') : c.best ? t('New Best') : c.many ? t('From Your Save') : t('Achievement Unlocked');
  document.getElementById('kicker').textContent = kicker;
  document.getElementById('title').textContent = c.title || '';
  const pts = c.points ? tf('{n} points', { n: c.points }) : '';
  const sub = document.getElementById('sub');
  sub.textContent = '';
  if (c.best) {
    const v = document.createElement('span');
    v.className = 'val';
    v.textContent = c.value || '';
    sub.append(v);
    if (c.rank) sub.append(` · #${c.rank}`);
  } else sub.textContent = c.mastered ? [t('Mastered'), pts].filter(Boolean).join(' · ') : c.beaten || c.event ? '' : [pts, c.casual ? t('Casual') : '', c.game || ''].filter(Boolean).join(' · ');
  document.getElementById('badge').textContent = c.beaten && !c.mastered ? t('Beaten') : t('Mastered');
  void el.offsetWidth;
  el.classList.add('in');
  if (c.sound) chime(big);
  await wait(big ? 5200 : 4000);
  el.classList.add('out');
  await wait(420);
  el.classList.remove('in', 'out');
}

// The tracker: the board's title, and a running clock for time boards, while an attempt is on.
const trk = document.getElementById('trk');
let trkNow = null, trkTick = 0, noteTimer = 0;
function setTracker(v) {
  clearInterval(trkTick);
  trkNow = v && v.title ? v : null;
  if (!trkNow) { trk.classList.remove('on'); return; }
  document.getElementById('trk-t').textContent = trkNow.title;
  document.getElementById('trk-n').textContent = '';
  const val = document.getElementById('trk-v');
  val.textContent = '';
  const fmt = trkNow.format || 'score';
  if (TIMED.includes(fmt) && trkNow.started) {
    // The clock runs in the board's own units, from when the attempt started.
    const per = fmt === 'millisecs' ? 1 : fmt === 'seconds' ? 1000 : fmt === 'minutes' ? 60000 : 1000 / 60;
    const tick = () => { val.textContent = fmtValue(Math.max(0, Date.now() - trkNow.started) / per, fmt); };
    tick();
    trkTick = setInterval(tick, fmt === 'seconds' || fmt === 'minutes' ? 500 : 50);
  }
  trk.classList.add('on');
}
function trackerNote(kind) {
  if (!trkNow || kind !== 'not_best') return;
  const n = document.getElementById('trk-n');
  n.textContent = t('Not your best');
  clearTimeout(noteTimer);
  noteTimer = setTimeout(() => { n.textContent = ''; }, 3000);
}

async function pump() {
  if (busy) return;
  busy = true;
  await ready;
  for (;;) {
    if (!queue.length) {
      try { queue.push(...(await invoke('ach_toast_take'))); } catch (e) { /* none */ }
    }
    if (!queue.length) break;
    await show(queue.shift());
  }
  busy = false;
  invoke('ach_toast_done').catch(() => {});
}

T.event.listen('ach-toast', () => pump());
T.event.listen('ach-tracker', (e) => { ready.then(() => setTracker(e.payload)); });
T.event.listen('ach-notice', (e) => { ready.then(() => trackerNote((e.payload || {}).kind)); });
ready.then(() => invoke('ach_tracker_get')).then(setTracker).catch(() => {});
pump();
