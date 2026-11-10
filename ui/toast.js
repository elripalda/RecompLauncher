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
async function show(c) {
  const big = !!c.mastered;
  el.classList.remove('in', 'out', 'big');
  el.classList.toggle('big', big);
  document.getElementById('kicker').textContent = big ? t('Every Achievement') : c.many ? t('From Your Save') : t('Achievement Unlocked');
  document.getElementById('title').textContent = c.title || '';
  const pts = c.points ? tf('{n} points', { n: c.points }) : '';
  document.getElementById('sub').textContent = big ? [t('Mastered'), pts].filter(Boolean).join(' · ') : [pts, c.casual ? t('Casual') : '', c.game || ''].filter(Boolean).join(' · ');
  document.getElementById('badge').textContent = t('Mastered');
  void el.offsetWidth;
  el.classList.add('in');
  if (c.sound) chime(big);
  await wait(big ? 5200 : 4000);
  el.classList.add('out');
  await wait(420);
  el.classList.remove('in', 'out');
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
pump();
