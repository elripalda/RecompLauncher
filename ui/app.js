'use strict';
// RecompLauncher's window. Everything it shows comes from the Rust side (src-tauri/src/lib.rs):
// the RecompLauncher catalog, each game's GitHub releases, and what's installed. Long jobs report back
// through two events: `install` (download progress) and `game` (a game started or closed).

const T = window.__TAURI__;
const invoke = (cmd, args) => T.core.invoke(cmd, args);

const GENRES = { action: N_('Action'), adventure: N_('Adventure'), platformer: N_('Platformers'), racing: N_('Racing'), fighting: N_('Fighting'), shooter: N_('Shooters'), sports: N_('Sports'), rpg: N_('RPG'), puzzle: N_('Puzzle') };
const OS_NAME = { windows: 'Windows', macos: 'Mac', linux: 'Linux' };
// Looks run on ReShade on Windows and on vkBasalt on Linux.
window.looksHere = () => S.os === 'windows' || S.os === 'linux';
const AI_USE = { code: N_('code'), art: N_('art'), text: N_('writing'), audio: N_('voice or music') };

const S = {
  os: 'windows', arch: 'x64', version: '', site: 'https://recomplauncher.com',
  library: {}, settings: {}, running: new Set(), settingUp: new Set(), jobs: {}, recent: [], roms: [], owned: {}, steam: false, pc: null, libs: [], shots: {},
  cat: null, offline: false, updates: {}, rels: {}, builds: {}, reports: {}, runCounts: {}, info: {}, mods: {}, modList: {}, news: null, modQ: '', setup: {}, looks: {}, gb: {}, modSrc: {}, easy: new Set(), shelf: {}, slide: 0, lq: '',
  view: 'home', arg: null, back: [], q: '', dev: {}, setTab: 'general',
  find: { genre: '', origin: '', sort: 'popular', mine: true, great: false, easy: false },
  lib: { sel: null, filter: 'all', sort: 'played' },
  tab: 'overview',
};

// ---------- little helpers ----------
const $ = (s, r = document) => r.querySelector(s);
const $$ = (s, r = document) => [...r.querySelectorAll(s)];
const esc = (s) => String(s == null ? '' : s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
// A translated sentence with markup in its gaps: the text is escaped, the values (already HTML) go in as they are.
const htmlFill = (s, v) => esc(s).replace(/\{(\w+)\}/g, (m, k) => (v && k in v ? (typeof v[k] === 'number' ? fmtNum(v[k]) : String(v[k])) : m));
// The RecompLauncher Discord (the catalog can name a newer invite).
const discordUrl = () => { const u = (S.cat && S.cat.site && S.cat.site.discord) || ''; return /^https:\/\//.test(u) ? u : 'https://discord.gg/FUzqGM2cvf'; };
const webUrl = (u) => (!u ? '' : /^(https?:|data:)/.test(u) ? u : S.site + (u[0] === '/' ? '' : '/') + u);
// Pictures come through the launcher's disk cache, so they show at once and work offline.
const media = (u) => { if (u && String(u).startsWith('local:')) return fileSrc(String(u).slice(6)); const w = webUrl(u); return !w || w.startsWith('data:') || !T.core.convertFileSrc ? w : T.core.convertFileSrc(w, 'rlimg'); };
const fmtN = (n) => (n >= 1e6 ? (n / 1e6).toFixed(n >= 1e7 ? 0 : 1).replace(/\.0$/, '') + 'M' : n >= 1e3 ? (n / 1e3).toFixed(n >= 1e4 ? 0 : 1).replace(/\.0$/, '') + 'K' : String(n || 0));
const SIZE_UNITS = [N_('{n} B'), N_('{n} KB'), N_('{n} MB'), N_('{n} GB'), N_('{n} TB')];
const fmtB = (b) => { if (!b) return ''; let i = 0; while (b >= 1024 && i < 4) { b /= 1024; i++; } return tf(SIZE_UNITS[i], { n: i > 1 ? fmtNum(Number(b.toFixed(b >= 100 ? 0 : 1))) : fmtNum(Math.round(b)) }); };
const fmtTime = (s) => {
  if (!s) return t('Not yet');
  const h = Math.floor(s / 3600), m = Math.round((s % 3600) / 60);
  if (!h) return tf('{n} min', { n: Math.max(1, m) });
  if (m && h < 10) return h === 1 ? tf('1 hour {m} min', { m }) : tf('{n} hours {m} min', { n: h, m });
  return tn(h, '1 hour', '{n} hours');
};
const playedFor = (g) => (!g || !g.last_played ? t('Not played yet') : g.played_secs < 60 ? t('Under a minute') : fmtTime(g.played_secs));
// On its own (a date column): "Today", not "today".
const agoCap = (ts) => ago(ts).replace(/^./, (c) => c.toLocaleUpperCase(curLocale()));
const ago = (ts) => {
  if (!ts) return '';
  const d = (Date.now() / 1000 - ts) / 86400;
  if (d < 1) return t('today'); if (d < 2) return t('yesterday'); if (d < 7) return t('this week'); if (d < 14) return t('last week'); if (d < 31) return t('this month');
  return new Date(ts * 1000).toLocaleDateString(curLocale(), { month: 'short', day: 'numeric', year: 'numeric' });
};
const date = (s) => { const d = new Date(s); return isNaN(d) ? '' : d.toLocaleDateString(curLocale(), { month: 'short', day: 'numeric', year: 'numeric' }); };
const catIcon = (k) => (GENRES[k] ? `<i class="cat-ico" style="--m:url(img/cat/${k}.png)" aria-hidden="true"></i>` : '');
const osIcon = (k) => `<i class="os-ico" style="--m:url(img/${k}.png)" title="${OS_NAME[k] || k}" aria-hidden="true"></i>`;
const originName = (id) => ((S.cat && S.cat.origins) || []).find((o) => o.id === id)?.name || id || '';
const isAI = (a) => !!(a && a.ai);
// Something the player has to do by hand before the first play, from the game's boot profile.
// A community server, not the game itself: it starts the server, so its button says so.
const isServer = (a) => !!(((a && a.boot) || (S.bootBuiltin || {})[a && a.id] || {}).server);
// A line on what the player's copy should look like, by the console it's from (How to Set Up).
function setupGuide(a) {
  const b = (a && a.boot) || (S.bootBuiltin || {})[a && a.id] || {};
  const w = String((b.files || {}).what || '');
  if (!w || /no game data/i.test(w)) return '';
  if (/\bN64\b|\.z64/i.test(w)) return t('Use your own ROM: .z64, .n64 and .v64 all work. When you pick it, the launcher checks it\'s the right game and version, then puts it where the game reads it.');
  if (/Xbox 360/i.test(w)) return t('Use a full, uncompressed .iso of your own disc, or its extracted files (the folder with default.xex at the top).');
  if (/\bPS3\b/i.test(w)) return t('Use your own disc dump (the folder that holds PS3_GAME) or a decrypted .iso. An update or license file comes from your own console or RPCS3.');
  if (/\bPS2\b|PlayStation 2/i.test(w)) return t('Use your own disc as an .iso file.');
  if (/GameCube|\bWii\b/i.test(w)) return t('Use your own disc as an .iso or .rvz file.');
  if (/\bPSP\b/i.test(w)) return t('Use your own UMD as an .iso, or its extracted files.');
  return '';
}
// More than one player (split-screen, co-op, online), from its README or its labels in the store.
const isMulti = (a) => !!((((a && a.boot) || (S.bootBuiltin || {})[a && a.id] || {}).multiplayer) || (a && Array.isArray(a.tags) && a.tags.some((x) => /multiplayer|co-op|split.?screen|online|netplay/i.test(String(x)))));
// A game's tags (Widescreen, 60 FPS, Online…), as the site lists them. They stay in English.
const tagsOf = (a) => (a && Array.isArray(a.tags) ? a.tags.filter((x) => typeof x === 'string' && x.trim()) : []);
// Facts about the original game (from IGDB), only the ones there are.
function aboutFacts(ab) {
  if (!ab) return [];
  const list = (v, n) => (Array.isArray(v) ? v.filter(Boolean).slice(0, n || 99).join(', ') : v || '');
  // A plain day ("2000-04-27") is that day wherever you are, not midnight in London.
  const day = (v) => { const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(String(v || '')); return m ? new Date(+m[1], +m[2] - 1, +m[3]).toLocaleDateString(curLocale(), { month: 'long', day: 'numeric', year: 'numeric' }) : /^\d{4}$/.test(String(v || '')) ? String(v) : date(v); };
  return [[t('Released'), ab.released ? day(ab.released) : ''], [t('Developer'), ab.developer], [t('Publisher'), ab.publisher], [t('Series'), ab.series], [t('Genre'), list(ab.genres)], [t('Modes'), list(ab.modes)], [t('First on'), list(ab.platforms, 5)]]
    .filter(([, v]) => v && String(v).trim());
}
// Other recomps a player of this one might like: shared tags count twice, then the same genre,
// series and console. Other versions of the same game and ones still to come are left out.
function moreLike(a, max = 12) {
  const tags = new Set(tagsOf(a)), series = a.about && a.about.series;
  const same = (x) => (a.game && x.game === a.game) || (a.title && x.title === a.title) || x.id === a.id;
  return visible().filter((x) => !x.soon && !same(x) && forOS(x)).map((x) => {
    let sc = tagsOf(x).filter((tg) => tags.has(tg)).length * 2;
    if (a.genre && x.genre === a.genre) sc += 3;
    if (a.origin && x.origin === a.origin) sc += 1.5;
    if (series && x.about && x.about.series === series) sc += 4;
    return [x, sc];
  }).filter(([, sc]) => sc >= 3).sort((p, q) => q[1] - p[1] || (q[0].downloads || 0) - (p[0].downloads || 0)).slice(0, max).map(([x]) => x);
}
const bootNote = (a) => String(((a && a.boot) || (S.bootBuiltin || {})[a && a.id] || {}).player_note || '');
invoke('boot_builtin').then((b) => { S.bootBuiltin = b || {}; }).catch(() => {});
const LOADED = new Set();
// A picture through the disk cache; one that has loaded before shows at once instead of fading in again.
const pic = (u, cls = '', more = '') => { const s = media(u); if (!s) return ''; const c = [cls, LOADED.has(s) ? 'ld' : ''].filter(Boolean).join(' '); return `<img src="${esc(s)}" alt=""${c ? ` class="${c}"` : ''}${more ? ' ' + more : ''} decoding="${LOADED.has(s) ? 'sync' : 'async'}">`; };
const cssUrl = (s) => String(s).replace(/['"()\\\s]/g, (c) => encodeURIComponent(c));
const baseName = (p) => String(p || '').split(/[\\/]/).filter(Boolean).pop() || String(p || '');
const pins = () => ((S.settings.ui && S.settings.ui.pins) || []).filter((id) => byId(id) || S.library[id]);
// The player's favourite games (Library filter, Home row).
const STAR = '<svg width="13" height="13" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true" style="margin-right:5px;vertical-align:-1px"><path d="m12 3.5 2.6 5.3 5.9.9-4.3 4.1 1 5.8L12 16.9l-5.2 2.7 1-5.8-4.3-4.1 5.9-.9z"/></svg>';
const favs = () => ((S.settings.ui && S.settings.ui.favs) || []).filter((id) => S.library[id]);
// A word that means different things in different places ("Light" as a theme, not a strength):
// its key carries the context after a bar, and English shows only the word.
const tc = (k) => { const v = t(k); return v === k ? k.split('|')[0] : v; };
const saveUi = (patch, quiet) => saveSettings({ ui: Object.assign({}, S.settings.ui || {}, patch) }, quiet);
function based(a) { return a.game ? `<div class="based">${DISC_S}<span>${htmlFill(t('Based on {game}'), { game: `<b>${esc(a.game)}</b>` })}${a.origin ? ` · ${esc(originName(a.origin))}` : ''}</span></div>` : ''; }
// A recomp that needs things on the PC first (Python, build tools, a long build): it says so, and
// its main button is a yellow Set Up with its developer's steps until the player says it's done.
const WRENCH = '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14.7 6.3a4 4 0 0 0 5 5L21 13l-8 8-3-3 8-8-1.3-1.3a4 4 0 0 1-5-5L9 2l3 3-2 2-3-3L5.3 5.7"/></svg>';
const needsSetup = (a) => !!(a && a.setup_required);
// Games added to the Library without installing them yet (like a store's library): id → when.
const saved = () => ((S.settings && S.settings.ui) || {}).saved || {};
const isSaved = (id) => !!saved()[id];
const PLUS = '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14"/></svg>';
async function addToLibrary(id) {
  await saveUi({ saved: Object.assign({}, saved(), { [id]: Math.floor(Date.now() / 1000) }) }, true);
  const a = byId(id);
  render(true);
  toast(tf('{name} is in your Library. Install it whenever you like.', { name: a ? a.name : id }));
}
async function removeFromLibrary(id) {
  const s2 = Object.assign({}, saved());
  delete s2[id];
  await saveUi({ saved: s2 }, true);
  // One added by its link goes away altogether.
  const a = byId(id);
  if (a && a.custom && !S.library[id]) { await invoke('remove_github', { id }).catch(() => {}); await loadCatalog(false); }
  render(true);
}
const setupDone = (id) => !!(((S.settings && S.settings.ui) || {}).setup_ok || {})[id];
const GITHUB_S = '<svg width="15" height="15" viewBox="0 0 24 24" aria-hidden="true"><path fill="currentColor" d="M12 1.8a10.2 10.2 0 0 0-3.2 19.9c.5.1.7-.2.7-.5v-1.8c-2.8.6-3.4-1.2-3.4-1.2-.5-1.2-1.1-1.5-1.1-1.5-.9-.6.1-.6.1-.6 1 .1 1.6 1 1.6 1 .9 1.6 2.4 1.1 3 .9.1-.7.4-1.1.6-1.4-2.3-.3-4.6-1.1-4.6-5 0-1.1.4-2 1-2.7-.1-.3-.4-1.3.1-2.7 0 0 .9-.3 2.8 1a9.6 9.6 0 0 1 5.1 0c1.9-1.3 2.8-1 2.8-1 .6 1.4.2 2.4.1 2.7.6.7 1 1.6 1 2.7 0 3.9-2.4 4.7-4.6 5 .4.3.7.9.7 1.9v2.8c0 .3.2.6.7.5A10.2 10.2 0 0 0 12 1.8z"/></svg>';
const DISC_S = '<svg class="disc" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" aria-hidden="true"><circle cx="12" cy="12" r="9"/><circle cx="12" cy="12" r="2.5"/></svg>';
const DISC_L = '<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><circle cx="12" cy="12" r="9"/><circle cx="12" cy="12" r="2.5"/><path d="M12 6a6 6 0 0 1 6 6" opacity=".5"/></svg>';
const CHEV_L = '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m15 5-7 7 7 7"/></svg>';
const CHEV_R = '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 5 7 7-7 7"/></svg>';
const SEARCH = '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/></svg>';
const XMARK = '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" aria-hidden="true"><path d="m6 6 12 12M18 6 6 18"/></svg>';
const CHECK = '<svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m5 12.5 4.5 4.5L19 7.5"/></svg>';
const aiUses = (a) => { const w = ((a.ai && a.ai.uses) || ['code']).map((u) => (AI_USE[u] ? t(AI_USE[u]) : u)); try { return new Intl.ListFormat(curLocale(), { type: 'conjunction' }).format(w); } catch (e) { return w.join(', '); } };

function toast(msg, bad, html, ms) {
  const box = $('#toasts');
  if (!box) return;
  if ([...box.children].some((el) => el.dataset.msg === String(msg))) return;
  const el = document.createElement('div');
  el.className = 'toast' + (bad ? ' bad' : '');
  el.dataset.msg = String(msg);
  if (html) el.innerHTML = html; else el.textContent = msg;
  box.append(el);
  while (box.children.length > 3) box.firstElementChild.remove();
  setTimeout(() => { el.classList.add('out'); setTimeout(() => el.remove(), 260); }, ms || (bad ? 7000 : 3800));
}
// "Checking for updates…" at the bottom, then what it found, in the same spot.
async function checkLauncher(btn) {
  const box = $('#toasts');
  if (!box || box.querySelector('.toast.checking')) return;
  if (btn) btn.disabled = true;
  const el = document.createElement('div');
  el.className = 'toast checking';
  el.setAttribute('role', 'status');
  el.innerHTML = `<i class="spin" aria-hidden="true"></i><span>${esc(t('Checking for updates…'))}</span>`;
  box.append(el);
  const t0 = Date.now();
  let v = null, bad = false;
  try { v = await invoke('launcher_update'); } catch { bad = true; }
  await new Promise((r) => setTimeout(r, Math.max(0, 900 - (Date.now() - t0))));
  if (btn) btn.disabled = false;
  if (v) { el.remove(); return updateToast(v); }
  el.classList.add('found');
  el.classList.toggle('bad', bad);
  el.innerHTML = bad ? `<span>${esc(t('Couldn\'t check for updates right now.'))}</span>` : `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m5 12.5 4.5 4.5L19 7.5"/></svg><span>${esc(t('You have the newest RecompLauncher.'))}</span>`;
  setTimeout(() => { el.classList.add('out'); setTimeout(() => el.remove(), 260); }, 3600);
}
const fail = (e) => toast(t(String(e && e.message ? e.message : e)), true);

// Markdown from README sections and release notes, made safe: HTML tags dropped, everything escaped,
// links opened in the browser.
function md(src) {
  const inline = (s) => s
    .replace(/!\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/`([^`]+)`/g, '<code>$1</code>')
    .replace(/\*\*([^*]+)\*\*/g, '<b>$1</b>').replace(/__([^_]+)__/g, '<b>$1</b>')
    .replace(/(^|[\s(])\*([^*\s][^*]*)\*/g, '$1<i>$2</i>').replace(/(^|[\s(])_([^_\s][^_]*)_(?=[\s.,)!?]|$)/g, '$1<i>$2</i>')
    .replace(/\[([^\]]+)\]\((https?:\/\/[^)\s]+)\)/g, '<a href="#" data-url="$2">$1</a>')
    .replace(/(^|[\s(])(https?:\/\/[^\s<)]+[^\s<).,;:!?])/g, '$1<a href="#" data-url="$2">$2</a>');
  const lines = esc(String(src || '').replace(/<!--[\s\S]*?-->/g, '').replace(/<[^>]+>/g, '').replace(/\r/g, '')).split('\n');
  const out = []; let para = [], list = null, code = null;
  const flush = () => { if (para.length) out.push('<p>' + inline(para.join(' ')) + '</p>'); para = []; if (list) { out.push(`</${list}>`); list = null; } };
  for (const raw of lines) {
    if (code !== null) { if (/^\s*```/.test(raw)) { out.push('<pre><code>' + code.join('\n') + '</code></pre>'); code = null; } else code.push(raw); continue; }
    if (/^\s*```/.test(raw)) { flush(); code = []; continue; }
    const l = raw.trimEnd();
    let m;
    if (!l.trim()) { flush(); continue; }
    if ((m = l.match(/^\s{0,3}(#{1,6})\s+(.*)$/))) { flush(); const n = Math.min(4, m[1].length + 1); out.push(`<h${n}>${inline(m[2].replace(/#+$/, ''))}</h${n}>`); continue; }
    if (/^\s*([-*_])(\s*\1){2,}\s*$/.test(l)) { flush(); continue; }
    if ((m = l.match(/^\s*(?:[-*+]|(\d+)[.)])\s+(.*)$/))) {
      if (para.length) { out.push('<p>' + inline(para.join(' ')) + '</p>'); para = []; }
      const kind = m[1] ? 'ol' : 'ul';
      if (list !== kind) { if (list) out.push(`</${list}>`); out.push(`<${kind}>`); list = kind; }
      out.push('<li>' + inline(m[2].replace(/^\[[ xX]\]\s*/, '')) + '</li>');
      continue;
    }
    if ((m = l.match(/^\s*&gt;\s?(.*)$/))) { flush(); out.push('<blockquote>' + inline(m[1]) + '</blockquote>'); continue; }
    if (list && /^\s{2,}/.test(raw)) { out[out.length - 1] = out[out.length - 1].replace(/<\/li>$/, ' ' + inline(l.trim()) + '</li>'); continue; }
    if (list) { out.push(`</${list}>`); list = null; }
    para.push(l.trim());
  }
  if (code !== null) out.push('<pre><code>' + code.join('\n') + '</code></pre>');
  flush();
  return out.join('');
}

// Titles never wrap or get cut off: a long one gets smaller, then its short form ("Banjo" for
// "Banjo: Recompiled"), then smaller still; past that its end fades and the line glides across on hover.
const shortName = (n) => String(n).replace(/[\s:-]+(recompiled|recompilation|recomp|pc port|port|decomp(ilation)?|for pc|pc)$/i, '').trim();
function fitOne(el) {
  if (el.dataset.full) el.textContent = el.dataset.full;
  el.style.fontSize = '';
  el.classList.remove('long');
  const w = el.clientWidth;
  if (!w) return;
  const base = parseFloat(getComputedStyle(el).fontSize);
  const shrink = (floor) => { let size = parseFloat(el.style.fontSize) || base; while (el.scrollWidth > w + 0.5 && size > floor) { size -= 0.5; el.style.fontSize = size + 'px'; } };
  shrink(13);
  if (el.scrollWidth > w + 0.5 && el.dataset.short) { el.textContent = el.dataset.short; el.style.fontSize = ''; shrink(13); }
  shrink(11.5);
  const over = el.scrollWidth - w;
  if (over > 0.5) {
    const txt = el.textContent;
    el.classList.add('long');
    el.innerHTML = `<span class="glide">${esc(txt)}</span>`;
    el.style.setProperty('--shift', -Math.ceil(over + 8) + 'px');
    el.style.setProperty('--glide', Math.max(1.2, over / 40).toFixed(2) + 's');
  }
}
function fitTitles(root = document) {
  for (const el of $$('.tile .t', root)) fitOne(el);
  // The small numbers on a game's page: a long date (Russian, Japanese) gets a little smaller
  // instead of cut off.
  for (const el of $$('.stats b', root)) {
    el.style.fontSize = '';
    let size = parseFloat(getComputedStyle(el).fontSize);
    while (el.clientWidth && el.scrollWidth > el.clientWidth + 0.5 && size > 12) { size -= 0.5; el.style.fontSize = size + 'px'; }
  }
}

// ---------- the catalog ----------
const all = () => (S.cat ? S.cat.apps : []);
// Discover and Home list the catalog's recomps; ones the player added by link live in their Library.
const visible = () => all().filter((a) => !a.custom && !(S.settings.hide_ai && isAI(a)));
// Looked up by id, and grouped by game: the catalog's titles put the recomps of one game together
// as its versions, the recommended one first, then the most downloaded.
let IDX = { key: '', cat: null, ids: new Map(), titles: new Map() };
function index() {
  const apps = all();
  const key = apps.length + '|' + Object.keys(S.library).length + '|' + Object.keys(S.builds || {}).length;
  if (IDX.cat !== S.cat || IDX.key !== key) {
    const ids = new Map(apps.map((a) => [a.id, a]));
    const titles = new Map();
    for (const ti of (S.cat && S.cat.titles) || []) {
      const every = (ti.pc || []).map((id) => ids.get(id)).filter((a) => a && a.title === ti.id);
      if (every.length < 2) continue;
      const pick = String(ti.pick || '').replace(/^pc:/, '');
      every.sort((x, y) => (y.id === pick) - (x.id === pick) || (x.soon ? 1 : 0) - (y.soon ? 1 : 0) || byDl(x, y));
      // Only the versions this computer can run (or that are already installed) are offered.
      const list = every.filter((a) => forOS(a) || S.library[a.id]);
      titles.set(ti.id, { id: ti.id, name: ti.name || every[0].game || every[0].name, pick: (list.find((a) => a.id === pick) || list[0] || every[0]).id, list, every });
    }
    const devs = new Map(((S.cat && S.cat.developers) || []).filter((d) => d && d.slug).map((d) => [d.slug, d]));
    IDX = { key, cat: S.cat, ids, titles, devs };
  }
  return IDX;
}
// A cover the player picked for a game (Change Cover) shows instead of the catalog's, and a name
// they gave it (Rename) instead of its own.
function applyCover(a, id) {
  if (!a) return a;
  const ui = S.settings.ui || {};
  // Show Game Names: the game's own name instead of the recomp's ("Tears of the Kingdom", not
  // "TotK Recomp"), unless the player named it themselves.
  const nm = (ui.names || {})[id] || (ui.game_names && a.game ? a.game : '');
  if (nm) { if (a._name === undefined) a._name = a.name; a.name = nm; } else if (a._name !== undefined) { a.name = a._name; delete a._name; }
  const c = (S.library[id] || {}).cover_file;
  if (c) { if (a._cov === undefined) a._cov = a.cover || ''; a.cover = 'local:' + c; } else if (a._cov !== undefined) { a.cover = a._cov; delete a._cov; }
  const h = (S.library[id] || {}).hero_file;
  if (h) { if (a._hero === undefined) a._hero = a.hero || ''; a.hero = 'local:' + h; } else if (a._hero !== undefined) { a.hero = a._hero; delete a._hero; }
  return a;
}
const byId = (id) => applyCover(index().ids.get(id), id);
// A game in the Library the catalog doesn't have (added by the player, a local build).
const libApp = (g) => byId(g.id) || { id: g.id, name: ((S.settings.ui || {}).names || {})[g.id] || g.name, repo: g.repo, systems: [S.os], cover: g.cover_file ? 'local:' + g.cover_file : '', hero: g.hero_file ? 'local:' + g.hero_file : '' };
// The developer behind a recomp, when they have an account: { slug, name, verified, avatar }.
const devOf = (a) => (a && a.dev ? index().devs.get(a.dev) || null : null);
// The signed-in account's developer page, when it's a verified developer. The website can say so
// itself (user.dev); until it does, a verified page with the same name as the account counts.
function devAccount() {
  const u = S.account && S.account.signedIn && S.account.user;
  if (!u) return null;
  const devs = index().devs;
  if (u.dev && u.dev.slug) { const d = devs.get(u.dev.slug); return (d && d.verified) || u.dev.verified ? Object.assign({}, u.dev, d || {}) : null; }
  const n = String(u.username || '').trim().toLowerCase();
  if (!n) return null;
  for (const d of devs.values()) if (d.verified && (String(d.slug).toLowerCase() === n || String(d.name || '').trim().toLowerCase() === n)) return d;
  return null;
}
// Developer tools are for verified developers (and anyone who already has a local build here).
const devAllowed = () => !!devAccount() || Object.values(S.library).some((g) => g.local && g.tag !== 'own');
const devOn = () => !!(S.settings.ui || {}).dev && devAllowed();
const TICK = (s = 16) => `<svg class="vtick" width="${s}" height="${s}" viewBox="0 0 24 24" role="img" aria-label="${esc(t('Verified developer'))}"><title>${esc(t('Verified developer'))}</title><path d="M12 2.5l2.4 1.8 3-.1.9 2.8 2.5 1.7-.9 2.9.9 2.8-2.5 1.8-.9 2.8-3-.1L12 21.5l-2.4-1.8-3 .1-.9-2.8-2.5-1.8.9-2.8-.9-2.9 2.5-1.7.9-2.8 3 .1z" fill="currentColor"/><path d="M8.3 12.2l2.5 2.5 5-5.1" fill="none" stroke="#0B1220" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>`;
// Who made it, under a game's name: their picture, their name, and the Verified badge.
function devLine(a) {
  const d = devOf(a);
  if (!d) return '';
  const name = d.name || a.author || '';
  return `<div class="dev-row"><button type="button" class="dev-line" data-act="url" data-url="${esc(SITE + '/dev/' + encodeURIComponent(d.slug))}" title="${esc(t('See their page'))}">${d.avatar ? `<img src="${esc(d.avatar)}" alt="" width="24" height="24" loading="lazy">` : `<span class="dev-ph">${esc(name.slice(0, 1).toUpperCase())}</span>`}<span>${htmlFill(t('By {who}'), { who: `<b>${esc(name)}</b>` })}</span>${d.verified ? `<span class="vpill">${TICK(14)}${esc(t('Verified developer'))}</span>` : ''}</button>${followBtn(d)}</div>`;
}
const follows = () => ((S.settings.ui || {}).follow || []);
const followBtn = (d) => { const on = follows().includes(d.slug); return `<button type="button" class="follow-btn${on ? ' on' : ''}" data-act="follow" data-dev="${esc(d.slug)}" aria-pressed="${on}">${on ? `<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m5 12.5 4.5 4.5L19 7.5"/></svg>${esc(t('Following'))}` : `<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14"/></svg>${esc(t('Follow'))}`}</button>`; };
async function toggleFollow(slug) {
  const d = index().devs.get(slug);
  const list = follows(), on = list.includes(slug);
  await saveUi({ follow: on ? list.filter((x) => x !== slug) : [...list, slug] }, true);
  toast(on ? tf('You stopped following {name}.', { name: (d && d.name) || slug }) : tf('Following {name}. You\'ll hear about their new recomps and versions.', { name: (d && d.name) || slug }));
  for (const b of $$(`.follow-btn[data-dev="${CSS.escape(slug)}"]`)) b.outerHTML = followBtn(d || { slug });
  if (S.view === 'home') render(true);
  if (window.tvRefresh) window.tvRefresh();
}
// A name with the tick after it, for lists.
const whoHTML = (a) => { const d = devOf(a); return esc((d && d.name) || a.author || '') + (d && d.verified ? TICK(13) : ''); };
// A game with more than one version for this computer: { id, name, pick, list }, or null. A page of
// a version that doesn't run here still points at the ones that do.
const verGroup = (a) => {
  const g = a && a.title ? index().titles.get(a.title) : null;
  return g && (g.list.length >= 2 || (g.list.length && !g.list.includes(a))) ? g : null;
};
// The version to show for a game: the one you play (installed, played last), else the
// recommended one, else the first that runs here.
// Recomps that put out a new version lately (not just their first one), each game once.
function recentlyUpdated(list) {
  const seen = new Set();
  return list.filter((a) => a.released && (Number(a.releases) || (a.releases || []).length || 0) > 1 && a.released !== a.first)
    .sort((x, y) => String(y.released).localeCompare(String(x.released)) || byDl(x, y))
    .filter((a) => { const k = a.title && index().titles.has(a.title) ? 't:' + a.title : a.id; if (seen.has(k)) return false; seen.add(k); return true; });
}
function leadOf(g, among = g.list) {
  const inst = among.filter((x) => S.library[x.id]).sort((x, y) => (S.library[y.id].last_played || 0) - (S.library[x.id].last_played || 0) || (S.library[y.id].installed_at || 0) - (S.library[x.id].installed_at || 0));
  return inst[0] || among.find((x) => x.id === g.pick) || among.find((x) => g.list.includes(x)) || among[0];
}
// Each game once in a list, where its first version comes. keep: show that first one itself
// (Newest, Recently updated), instead of the one you'd play.
function oneEach(list, keep) {
  const T = index().titles, groups = new Map();
  for (const a of list) { const g = a.title && T.get(a.title); if (g) { if (!groups.has(g.id)) groups.set(g.id, []); groups.get(g.id).push(a); } }
  if (!groups.size) return list;
  const out = [];
  for (const a of list) {
    const g = a.title && T.get(a.title);
    if (!g) { out.push(a); continue; }
    const here = groups.get(g.id);
    if (!here) continue;
    groups.delete(g.id);
    out.push(keep && g.list.includes(a) ? a : leadOf(g, here));
  }
  return out;
}
// Installed, this version or another of the same game.
const haveAny = (a) => !!(a && (S.library[a.id] || (verGroup(a) && verGroup(a).list.some((v) => S.library[v.id]))));
// The version to show for a game: the one you play (installed, played last), else the
// recommended one, else the first.
function leadOf(g, among = g.list) {
  const inst = among.filter((x) => S.library[x.id]).sort((x, y) => (S.library[y.id].last_played || 0) - (S.library[x.id].last_played || 0) || (S.library[y.id].installed_at || 0) - (S.library[x.id].installed_at || 0));
  return inst[0] || among.find((x) => x.id === g.pick) || among[0];
}
// Each game once in a list, where its first version comes. keep: show that first one itself
// (Newest, Recently updated), instead of the one you'd play.
function oneEach(list, keep) {
  const groups = new Map();
  for (const a of list) { const g = verGroup(a); if (g) { if (!groups.has(g.id)) groups.set(g.id, []); groups.get(g.id).push(a); } }
  if (!groups.size) return list;
  const out = [];
  for (const a of list) {
    const g = verGroup(a);
    if (!g) { out.push(a); continue; }
    const here = groups.get(g.id);
    if (!here) continue;
    groups.delete(g.id);
    out.push(keep ? a : leadOf(g, here));
  }
  return out;
}
const forOS = (a) => (a.systems || []).includes(S.os) || !!(a.assets && a.assets[S.os]) || !!S.builds[a.id] || viaProton(a);
// On Linux with Proton on, a recomp with only a Windows version plays through Proton.
const viaProton = (a) => S.os === 'linux' && !!S.settings.proton && ((a.systems || []).includes('windows') || !!(a.assets && a.assets.windows));
const byDl = (x, y) => (y.downloads || 0) - (x.downloads || 0);
const byNew = (x, y) => String(y.added || y.released || '').localeCompare(String(x.added || x.released || ''));
const byUpdated = (x, y) => String(y.released || '').localeCompare(String(x.released || ''));
// Discover's Show filter: everything, what's installed, what isn't, or what needs setting up first.
const SHOWS = [['', N_('Everything')], ['inst', N_('Installed')], ['notinst', N_('Not installed')], ['lib', N_('In your Library')], ['setup', N_('Setup Required')]];
function showOk(a, k) {
  if (!k) return true;
  if (k === 'inst') return !!S.library[a.id];
  if (k === 'notinst') return !S.library[a.id];
  if (k === 'lib') return !!S.library[a.id] || isSaved(a.id);
  if (k === 'setup') return needsSetup(a) && !setupDone(a.id);
  return true;
}

function status(a) {
  const id = a.id, job = S.jobs[id], inst = S.library[id];
  if (S.running.has(id)) return 'running';
  if (S.settingUp.has(id)) return 'settingup';
  if (job) return 'busy';
  if (inst && S.updates[id]) return 'update';
  if (inst) return 'installed';
  if (a.soon) return 'soon';
  if (!forOS(a)) return 'nobuild';
  return 'get';
}

// ---------- pieces ----------
function poster(a, mine, inst, wide) {
  const st = status(a), job = S.jobs[a.id];
  const own = !S.library[a.id] && S.owned[a.id];
  const badge = S.missing && S.missing.has(a.id) ? `<span class="badge tr miss">${esc(t('Missing'))}</span>`
    : st === 'update' ? `<span class="badge tr">${esc(t('Update'))}</span>`
    : st === 'running' ? `<span class="badge tr">${esc(t('Playing'))}</span>`
      : st === 'soon' ? `<span class="badge tr soon">${esc(t('Soon'))}</span>`
        : st === 'installed' && !mine ? `<span class="badge tl">${esc(t('Installed'))}</span>`
          : !S.library[a.id] && isSaved(a.id) && !mine ? `<span class="badge tl lib">${esc(t('In Library'))}</span>`
            : own ? `<span class="badge tl">${esc(t('You have it'))}</span>` : '';
  const lb = S.library[a.id] && S.library[a.id].local ? `<span class="badge tl dev">${esc(S.library[a.id].tag === 'own' ? t('Added by You') : t('Local build'))}</span>` : '';
  const easy = needsSetup(a) && !a.soon && !setupDone(a.id) ? `<span class="badge easy setupreq">${WRENCH.replace(/18/g, '13')}${esc(t('Setup Required'))}</span>`
    : !S.library[a.id] && S.easy.has(a.id) && !a.soon ? `<span class="badge easy">${DISC_S}${esc(t('Easy Setup'))}</span>` : '';
  const prog = job ? `<span class="bar-p"><i data-prog-bar="${esc(a.id)}" style="width:${pct(job)}%"></i></span>` : '';
  const vg = !mine && !prog && verGroup(a);
  const vers = vg ? `<span class="badge vers" title="${esc(tn(vg.list.length, '1 version', '{n} versions'))}">${LAYERS}${fmtNum(vg.list.length)}</span>`
    : inst > 1 && !prog ? `<span class="badge tl" title="${esc(tn(inst, '1 installed', '{n} installed'))}">${LAYERS}${esc(tn(inst, '1 installed', '{n} installed'))}</span>` : '';
  if (wide) {
    // A wide tile: the game's art, blurred and darkened, with its logo in the middle (its name
    // when there's no logo).
    const art = a.hero || a.cover, logo = a.logo;
    return `<div class="poster wide${lb ? ' local' : ''}"><span class="ph"></span>${art ? `<img class="wbg" src="${esc(media(art))}" alt="" loading="lazy" decoding="async">` : ''}<span class="wshade"></span>${logo ? `<img class="wlogo" src="${esc(media(logo))}" alt="${esc(a.name)}" loading="lazy" decoding="async">` : `<span class="wname">${esc(a.name)}</span>`}${lb || badge}${prog ? prog : easy}${vers}</div>`;
  }
  return `<div class="poster${lb ? ' local' : ''}"><span class="ph">${esc(a.name)}</span>${pic(a.cover, '', 'loading="lazy"')}${lb || badge}${prog ? prog : easy}${vers}</div>`;
}

function tile(a, opts = {}) {
  const cov = a.cover ? media(a.cover) : '';
  const sub = opts.sub != null ? opts.sub : a.game ? `${DISC_S}<span>${esc(a.game)}</span>` : `<span>${esc(originName(a.origin))}</span>`;
  const vt = (devOf(a) || {}).verified;
  const quick = opts.quick && S.library[a.id] && !S.jobs[a.id];
  // Not in the Library yet: add it from the cover; added: install from the cover.
  const qadd = !quick && !opts.mine && !S.library[a.id] && !S.jobs[a.id] && status(a) === 'get';
  const qsaved = qadd && isSaved(a.id);
  return `<div class="tile${opts.on ? ' on' : ''}" data-id="${esc(a.id)}">${cov ? `<span class="glow" style="background-image:url('${cssUrl(cov)}')"></span>` : ''}
    <button type="button" class="tile-b" data-act="${opts.act || 'open'}" data-id="${esc(a.id)}" aria-label="${esc(a.name)}">${poster(a, opts.mine, opts.inst, opts.wide)}${vt ? '<span class="trow">' : ''}<span class="t"${shortName(a.name) !== a.name ? ` data-short="${esc(shortName(a.name))}" data-full="${esc(a.name)}"` : ''} title="${esc(a.name)}">${esc(a.name)}</span>${vt ? `${TICK(15)}</span>` : ''}<span class="s">${sub}</span></button>
    ${quick ? `<span class="qpw"><button type="button" class="qp" data-act="${S.running.has(a.id) ? 'resume' : 'play'}" data-id="${esc(a.id)}" aria-label="${esc(tf('Play {name}', { name: a.name }))}" title="${esc(t('Play'))}">${PLAY}</button></span>` : ''}${qadd ? `<span class="qpw"><button type="button" class="qp${qsaved ? '' : ' add'}" data-act="${qsaved ? 'install' : 'addlib'}" data-id="${esc(a.id)}" aria-label="${esc(qsaved ? tf('Install {name}', { name: a.name }) : tf('Add {name} to your Library', { name: a.name }))}" title="${esc(qsaved ? t('Install') : t('Add to Library'))}">${qsaved ? DOWN : PLUS}</button></span>` : ''}</div>`;
}

const pct = (j) => (j && j.total ? Math.min(100, Math.round((j.done / j.total) * 100)) : 0);
function progLabel(j) {
  if (!j) return '';
  if (j.phase === 'prepare') return [t('Getting ready'), ''];
  if (j.phase === 'queued') return [t('Waiting for the download before it'), ''];
  if (j.phase === 'backup') return [t('Backing up your saves'), ''];
  if (j.phase === 'verify') return [t('Checking the download'), ''];
  if (j.phase === 'unpack') return [t('Unpacking'), ''];
  if (j.phase === 'clone') return [t('Getting its project files'), ''];
  return [tf('{done} of {total}', { done: fmtB(j.done), total: fmtB(j.total) }), j.speed ? tf('{speed}/s', { speed: fmtB(j.speed) }) : ''];
}
function progress(id) {
  const j = S.jobs[id];
  const [l, r] = progLabel(j);
  const busy = !j || j.phase !== 'download' || !j.total;
  // A backup that takes too long can be skipped: the update goes on without it.
  const right = j && j.phase === 'backup' ? `<button type="button" class="link-btn" data-act="skip-backup">${esc(t('Skip'))}</button>` : esc(r);
  return `<div class="prog" data-prog="${esc(id)}"><div class="track${busy ? ' busy' : ''}"><i style="width:${pct(j)}%"></i></div><div class="lbl"><span>${esc(l)}</span><span>${right}</span></div></div>`;
}

const HEART = '<svg width="18" height="18" viewBox="0 0 24 24" aria-hidden="true"><path fill="#FF5E7A" d="M12 21s-7.5-4.6-9.6-9.3C.9 8.2 3 4.5 6.6 4.5c2.1 0 3.5 1.2 4.4 2.6.9-1.4 2.3-2.6 4.4-2.6 3.6 0 5.7 3.7 4.2 7.2C19.5 16.4 12 21 12 21z"/></svg>';
const KOFI = 'https://ko-fi.com/recomp';
const DISCORD = '<svg width="22" height="22" viewBox="0 0 24 24" aria-hidden="true"><path fill="currentColor" d="M19.3 5.4A16.6 16.6 0 0 0 15.2 4l-.5 1a15.4 15.4 0 0 0-5.4 0l-.5-1a16.5 16.5 0 0 0-4.1 1.4C2.1 9.3 1.4 13.1 1.8 16.9a16.7 16.7 0 0 0 5 2.6l1.1-1.7a10.8 10.8 0 0 1-1.7-.8l.4-.3a11.9 11.9 0 0 0 10.8 0l.4.3c-.5.3-1.1.6-1.7.8l1.1 1.7a16.6 16.6 0 0 0 5-2.6c.5-4.4-.8-8.2-2.9-11.5zM8.7 14.6c-1 0-1.8-.9-1.8-2s.8-2 1.8-2 1.8.9 1.8 2-.8 2-1.8 2zm6.6 0c-1 0-1.8-.9-1.8-2s.8-2 1.8-2 1.8.9 1.8 2-.8 2-1.8 2z"/></svg>';
// What the controller buttons do, for Settings > Controllers (desktop and Big Picture mode).
const PAD_NAME = { a: 'A', b: 'B', x: 'X', y: 'Y', lb: 'LB', rb: 'RB', view: N_('View'), menu: N_('Menu') };
const PAD_PS = { a: N_('Cross'), b: N_('Circle'), x: N_('Square'), y: N_('Triangle'), lb: 'L1', rb: 'R1', view: N_('Create'), menu: N_('Options') };
const padKind = () => S.padKind || ((S.pads || [])[0] || {}).kind || 'xbox';
const PAD_NIN = { a: 'A', b: 'B', x: 'X', y: 'Y', lb: 'L', rb: 'R', view: N_('Minus'), menu: N_('Plus') };
// The icon file and button names for a kind of controller: xbox, ps or nintendo.
window.padArt = (k, n) => { const ps = k === 'ps', nin = k === 'nintendo'; return { url: `img/pad/${ps ? 'ps' : nin ? 'nintendo' : 'xbox'}-${n}.${ps ? 'png' : 'svg'}`, name: (ps ? PAD_PS : nin ? PAD_NIN : PAD_NAME)[n] || n }; };
const padGlyph = (n) => { const p = padArt(padKind(), n); return `<i class="glyph g-${n}" style="--m:url(${p.url})" role="img" aria-label="${esc(t(p.name))}"></i>`; };
function buttonGuide(g = padGlyph) {
  const plus = '<em>+</em>';
  return [
    [`<small>${esc(t('Hold'))}</small>${g('view')}`, t('Quick menu'), t('While you play. Ctrl+F12 on a keyboard.')],
    [g('view') + plus + g('rb'), t('Screenshot'), t('While you play. F12 on a keyboard.')],
    [`<small>${esc(t('Hold'))}</small>${g('menu')}`, t('Big Picture mode'), t('Opens it from the desktop view.')],
    [g('lb') + g('rb'), t('Sections'), t('Moves between Home, Library and the rest.')],
    [g('a') + g('b'), t('Select and back'), t('Opens, picks and changes things, or goes back.')],
    [g('y'), t('Game page'), t('On a game, opens its page.')],
  ];
}
window.buttonGuide = buttonGuide;
// 0.75.0-beta.1 as people say it: 0.75 Beta 1.
function verName(v) {
  const m = /^v?(\d+)\.(\d+)(?:\.(\d+))?(?:-(beta|test)\.?(\d+))?/i.exec(String(v || ''));
  if (!m) return String(v || '');
  const base = m[3] && m[3] !== '0' ? `${m[1]}.${m[2]}.${m[3]}` : `${m[1]}.${m[2]}`;
  if (!m[5]) return base;
  return m[4].toLowerCase() === 'test' ? tf('{version} Test {b}', { version: base, b: m[5] }) : tf('{version} Beta {b}', { version: base, b: m[5] });
}
const LAYERS = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m12 3 9 5-9 5-9-5z"/><path d="m3 13 9 5 9-5"/></svg>';
const PLAY = '<svg width="18" height="18" viewBox="0 0 24 24" aria-hidden="true"><path d="M7 4.8v14.4a.8.8 0 0 0 1.2.7l11.5-7.2a.8.8 0 0 0 0-1.4L8.2 4.1A.8.8 0 0 0 7 4.8z" fill="currentColor"/></svg>';
const DOWN = '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 4v11m0 0-4.5-4.5M12 15l4.5-4.5M5 20h14"/></svg>';
const MORE = '<svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><circle cx="5" cy="12" r="1.8"/><circle cx="12" cy="12" r="1.8"/><circle cx="19" cy="12" r="1.8"/></svg>';

// The main button for a game, whatever state it's in.
function actions(a, size = 'big') {
  const st = status(a), id = esc(a.id), sq = size === 'big' ? 'btn sq glass' : 'btn sq sm', more = `<button type="button" class="${sq}" data-act="menu" data-id="${id}" aria-label="${esc(t('More'))}" title="${esc(t('More'))}">${MORE}</button>`;
  const b = (cls, act, label, icon = '') => `<button type="button" class="btn ${size} ${cls}" data-act="${act}" data-id="${id}">${icon}${label}</button>`;
  switch (st) {
    case 'running': return b('main', 'resume', esc(t('Back to the game')), PLAY) + b('', 'stop', esc(t('Close game'))) + more;
    case 'settingup': return `<button type="button" class="btn ${size}" disabled>${esc(t('Its setup is open'))}</button>` + more;
    case 'busy': return progress(a.id) + `<button type="button" class="${sq}" data-act="cancel" data-id="${id}" aria-label="${esc(t('Cancel'))}" title="${esc(t('Cancel'))}"><svg width="14" height="14" viewBox="0 0 12 12" aria-hidden="true"><path d="m1.5 1.5 9 9m0-9-9 9" stroke="currentColor" stroke-width="1.6"/></svg></button>`;
    case 'update': return b('main', 'install', esc(t('Update')), DOWN) + b('', 'play', esc(t('Play')), PLAY) + more;
    case 'installed': return needsSetup(a) && !setupDone(a.id) ? b('main warn', 'setupreq', esc(t('Set Up')), WRENCH) + more : S.easy.has(a.id) && S.setup[a.id] && S.setup[a.id].needs && (!S.setup[a.id].copy || needUpdate(S.setup[a.id])) ? b('main', 'setup', esc(t('Set up')), DISC_S.replace(/14/g, '18')) + more : isServer(a) ? b('main', 'play', esc(t('Start Server')), PLAY) + more : size === 'big' && otherInstalled(a).length ? `<span class="split">${b('main', 'play', esc(t('Play')), PLAY)}<button type="button" class="btn ${size} main caret" data-act="play-pick" data-id="${id}" aria-label="${esc(t('Play another version'))}" title="${esc(t('Play another version'))}"><svg width="14" height="14" viewBox="0 0 12 12" aria-hidden="true"><path d="m2.5 4.5 3.5 3.5 3.5-3.5" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/></svg></button></span>` + more : b('main', 'play', esc(t('Play')), PLAY) + more;
    case 'soon': return `<button type="button" class="btn ${size}" disabled>${esc(t('Coming soon'))}</button>` + more;
    case 'nobuild': return `<button type="button" class="btn ${size}" disabled>${esc(tf('No {os} version yet', { os: OS_NAME[S.os] }))}</button>` + more;
    default: return (isSaved(a.id) ? b('main', 'install', esc(t('Install')) + (a.size ? ` <span style="opacity:.7;font-weight:600">· ${esc(a.size)}</span>` : ''), DOWN) : b('main', 'addlib', esc(t('Add to Library')), PLUS)) + (needsSetup(a) && size === 'big' ? b('warn-o', 'setupreq', esc(t('Setup Required')), WRENCH) : '') + more;
  }
}

// The other versions of this game that are installed too.
const otherInstalled = (a) => { const g = a && a.title && index().titles.get(a.title); return g ? g.every.filter((v) => v.id !== a.id && S.library[v.id]) : []; };
// A checkmark, a warning, or (null) a grey "don't know".
const okMark = (v) => (v ? `<span class="mark ok" title="${esc(t('Yes'))}"><svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round" aria-label="${esc(t('Yes'))}"><path d="m5 12.5 4.5 4.5L19 7.5"/></svg></span>`
  : v === null ? `<span class="mark maybe" title="${esc(t('Not sure'))}"><svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" aria-label="${esc(t('Not sure'))}"><path d="M7 12h10"/></svg></span>`
    : `<span class="mark no" title="${esc(t('Missing'))}"><svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" aria-label="${esc(t('Missing'))}"><path d="M12 7v6M12 17h.01"/></svg></span>`);

// Before playing: is everything this recomp needs here? Your copy of the game, the graphics
// driver, the processor, the Windows runtime, and room on the drive.
function readyRows(a) {
  const id = a.id, g = S.library[id], pc = S.pc || {}, rows = [];
  const own = S.owned[id], su = S.setup[id];
  if (su && su.needs) rows.push({ k: t('Your copy of the game'), v: su.copy ? true : own ? null : null,
    d: su.copy ? tf('Set up with {file}', { file: baseName(su.copy) }) : own ? tf('Found in your folders: {file}', { file: baseName(own.path) }) : t('Insert it once and the launcher sets the game up for you.'),
    fix: su.copy ? '' : `<button type="button" class="btn sm main" data-act="setup" data-id="${esc(id)}">${esc(own ? t('Set it up') : t('Insert it'))}</button>` });
  else if (!g && S.easy.has(id)) rows.push({ k: t('Your copy of the game'), v: own ? true : null,
    d: own ? tf('Found in your folders: {file}. It\'s set up for you after it installs.', { file: baseName(own.path) }) : t('You insert it after it installs, and the launcher sets the game up.'), fix: '' });
  else if (a.needs) rows.push({ k: t('Your copy of the game'), v: own ? true : g && g.last_played ? true : null,
    d: own ? (own.region ? tf('Found: {path} ({region})', { path: own.path, region: own.region }) : tf('Found: {path}', { path: own.path })) : g && g.last_played ? t('Set up the first time you played') : t('The recomp asks for it the first time it starts. Add your game folders in Settings and the launcher finds it for you.'),
    fix: own || (g && g.last_played) ? '' : `<button type="button" class="btn sm" data-go="settings">${esc(t('Add folders'))}</button>` });
  if (su && su.needs && su.update) rows.push({ k: t('The game\'s update'), v: su.update_copy ? true : su.update_optional ? true : null,
    d: su.update_copy ? tf('Set up with {file}', { file: baseName(su.update_copy) }) : su.update,
    fix: su.update_copy || su.update_optional ? '' : `<button type="button" class="btn sm main" data-act="setupdate" data-id="${esc(id)}">${esc(t('Choose It'))}</button>` });
  if (pc.vulkan != null) rows.push({ k: t('Graphics (Vulkan)'), v: pc.vulkan, d: pc.vulkan ? t('Your graphics driver has it') : t('Update your graphics driver to get it'), fix: pc.vulkan ? '' : `<button type="button" class="btn sm" data-act="url" data-url="https://www.vulkan.org/tools#download-these-essential-development-tools">${esc(t('How'))}</button>` });
  if (a.origin === 'x360') rows.push({ k: t('Processor (AVX2)'), v: !!pc.avx2, d: pc.avx2 ? t('Your processor has it') : t('Xbox 360 recomps need a processor from 2013 or later'), fix: '' });
  // A recomp whose own setup builds it on this PC (from a clone of its repo).
  const builds = !!((a.boot && a.boot.clone) || ((S.bootBuiltin || {})[id] || {}).clone);
  if (builds && pc.build_tools != null) rows.push({ k: t('C++ build tools'), v: pc.build_tools, d: pc.build_tools ? t('Installed') : t('Its setup builds the game on your PC with Visual Studio\'s free C++ build tools'), fix: pc.build_tools ? '' : `<button type="button" class="btn sm main" data-act="url" data-url="https://visualstudio.microsoft.com/visual-cpp-build-tools/">${esc(t('Get them'))}</button>` });
  if (pc.vcredist != null) rows.push({ k: t('Visual C++ runtime'), v: pc.vcredist, d: pc.vcredist ? t('Installed') : t('Almost every Windows recomp needs it'), fix: pc.vcredist ? '' : `<button type="button" class="btn sm main" data-act="vcredist">${esc(t('Get it'))}</button>` });
  if (!g && a.size) {
    const lib = (S.libs || []).find((l) => l.default);
    const need = parseSize(a.size) * 3;
    if (lib && lib.free != null && need) rows.push({ k: t('Room on the drive'), v: lib.free > need, d: tf('{free} free in {path}', { free: fmtB(lib.free), path: lib.path }), fix: '' });
  }
  return rows;
}
const parseSize = (sz) => { const m = String(sz || '').match(/([\d.]+)\s*(KB|MB|GB)/i); return m ? parseFloat(m[1]) * { KB: 1024, MB: 1048576, GB: 1073741824 }[m[2].toUpperCase()] : 0; };

function readyCard(a) {
  const rows = readyRows(a);
  if (!rows.length) return '';
  const bad = rows.filter((r) => r.v === false || (r.v === null && r.fix)).length;
  return `<div class="card ready"><div class="card-h"><h3>${esc(bad || !S.library[a.id] ? t('Before you play') : t('Ready to play'))}</h3><span>${esc(bad ? tn(bad, '1 thing to do', '{n} things to do') : t('Everything it needs is here'))}</span></div>
    ${rows.map((r) => `<div class="set-row"><div class="k"><b>${esc(r.k)}</b><span>${esc(r.d)}</span></div><div class="acts">${r.fix}${r.fix && r.v !== true ? '' : okMark(r.v)}</div></div>`).join('')}</div>`;
}

// Play time from the sessions the launcher counted: this week, this month, and the last two weeks.
function playCard(g) {
  const ses = (g && g.sessions) || [];
  if (!ses.length) return '';
  const now = Date.now() / 1000, sum = (from) => ses.filter((x) => x[0] >= from).reduce((acc, x) => acc + x[1], 0);
  const longest = Math.max(...ses.map((x) => x[1]));
  const days = [];
  for (let i = 13; i >= 0; i--) {
    const d = new Date(); d.setHours(0, 0, 0, 0); d.setDate(d.getDate() - i);
    const from = d.getTime() / 1000, to = from + 86400;
    days.push({ d, secs: ses.filter((x) => x[0] >= from && x[0] < to).reduce((acc, x) => acc + x[1], 0) });
  }
  const max = Math.max(60, ...days.map((x) => x.secs));
  const recent = days.some((x) => x.secs);
  return `<div class="card"><div class="card-h" style="margin-bottom:14px"><h3>${esc(t('Your play time'))}</h3><span>${esc(t('The last two weeks'))}</span></div>
    <div class="stats">${[[t('This week'), fmtTime(sum(now - 7 * 86400))], [t('This month'), fmtTime(sum(now - 30 * 86400))], [t('Sessions'), fmtNum(ses.length)], [t('Longest'), fmtTime(longest)]].map(([k, v]) => `<div><span>${esc(k)}</span><b>${esc(v)}</b></div>`).join('')}</div>
    ${recent ? '' : `<p class="muted" style="margin:14px 0 0;font-size:14px">${esc(t('Not played in the last two weeks.'))}</p>`}<div class="bars${recent ? '' : ' none'}" role="img" aria-label="${esc(t('Play time over the last two weeks'))}">${days.map((x) => `<div class="b" title="${esc(tf('{day}: {time}', { day: x.d.toLocaleDateString(curLocale(), { weekday: 'short', month: 'short', day: 'numeric' }), time: x.secs ? fmtTime(x.secs) : t('not played') }))}"><i style="height:${x.secs ? Math.max(6, Math.round((x.secs / max) * 100)) : 0}%"></i><span>${esc(x.d.toLocaleDateString(curLocale(), { weekday: 'narrow' }))}</span></div>`).join('')}</div></div>`;
}

// About the original game, from Wikipedia.
async function loadInfo(id) {
  if (S.info[id] !== undefined) return;
  S.info[id] = 'loading';
  try { S.info[id] = await invoke('game_info', { id }); } catch (e) { S.info[id] = null; }
  if (S.view === 'game' && S.arg === id) refreshGame();
  if (window.tvRefresh) window.tvRefresh();
}
// Screenshots from the recomp's own page, in an even grid; a click shows one big.
function devShots(a) { return (Array.isArray(a.screenshots) ? a.screenshots : []).filter((u) => /^https:\/\//.test(u)); }
function shotsCard(a) {
  const all = devShots(a);
  if (!all.length) return '';
  const n = all.length >= 6 ? 6 : all.length === 5 ? 4 : all.length;
  const cols = n === 4 ? 2 : Math.min(n, 3);
  return `<div class="card devshots-card"><h3 style="margin-bottom:12px">${esc(t('Screenshots'))}</h3><div class="devshots" style="grid-template-columns:repeat(${cols},minmax(0,1fr))">${all.slice(0, n).map((u, i) => `<button type="button" class="devshot" data-act="devshot" data-id="${esc(a.id)}" data-i="${i}" aria-label="${esc(tf('Screenshot {n}', { n: i + 1 }))}">${pic(u, '', 'loading="lazy"')}</button>`).join('')}</div></div>`;
}
function devShotView(id, i) {
  const all = devShots(byId(id) || {});
  if (!all.length) return;
  let at = Math.max(0, Math.min(all.length - 1, i));
  const draw = (box) => { const im = box.querySelector('.devshot-big'); if (im) im.innerHTML = pic(all[at]); const c = box.querySelector('.devshot-n'); if (c) c.textContent = `${at + 1} / ${all.length}`; };
  const m = modal(`<div class="devshot-big"></div><div class="acts devshot-acts"><button type="button" class="btn" data-x="prev"${all.length > 1 ? '' : ' disabled'}>‹</button><span class="devshot-n"></span><button type="button" class="btn" data-x="next"${all.length > 1 ? '' : ' disabled'}>›</button><button type="button" class="btn main" data-x="no">${esc(t('Close'))}</button></div>`, (x) => {
    if (x.dataset.x === 'prev') { at = (at - 1 + all.length) % all.length; draw(m); }
    if (x.dataset.x === 'next') { at = (at + 1) % all.length; draw(m); }
  }, t('Screenshots'));
  m.classList.add('devshot-modal');
  m.addEventListener('keydown', (e) => {
    if (all.length < 2 || !['ArrowLeft', 'ArrowRight'].includes(e.key)) return;
    at = (at + (e.key === 'ArrowRight' ? 1 : all.length - 1)) % all.length; draw(m);
  });
  draw(m);
}
function aboutCard(a) {
  if (a.about && (a.about.summary || aboutFacts(a.about).length)) return igdbCard(a.about);
  const i = S.info[a.id];
  if (i === undefined) loadInfo(a.id);
  if (!i || i === 'loading') return '';
  return `<div class="card about-game"><div class="card-h" style="margin-bottom:10px"><h3>${esc(tf('About {title}', { title: i.title }))}</h3><span>${esc(i.kind)}</span></div>
    ${i.text.split('\n\n').map((p) => `<p>${esc(p)}</p>`).join('')}
    <p class="src">${esc(t('From Wikipedia, under CC BY-SA.'))} <a href="#" data-url="${esc(i.url)}">${esc(t('Read the whole article'))}</a></p></div>`;
}

// About the original game, from IGDB: what it is, then the facts there are.
function igdbCard(ab) {
  const facts = aboutFacts(ab), score = Number.isFinite(Number(ab.rating)) && ab.rating !== null && ab.rating !== '' ? Math.round(Number(ab.rating)) : null;
  return `<div class="card about-game igdb"><div class="card-h" style="margin-bottom:10px"><h3>${esc(t('About the game'))}</h3>${score !== null ? `<span class="score-b" title="${esc(t('Critic and player score'))}"><b>${score}</b>${esc(t('Critic and player score'))}</span>` : ''}</div>
    ${ab.summary ? String(ab.summary).split(/\n\s*\n/).map((p) => `<p dir="auto">${esc(p.trim())}</p>`).join('') : ''}
    ${facts.length ? `<dl class="dl facts">${facts.map(([k, v]) => `<div><dt>${esc(k)}</dt><dd>${esc(v)}</dd></div>`).join('')}</dl>` : ''}
    ${ab.url && /^https:\/\//.test(ab.url) ? `<p class="src"><a href="#" data-url="${esc(ab.url)}">${esc(t('Game info from IGDB'))}</a></p>` : ''}</div>`;
}

// What's new for the games you have: their releases from the last two months.
async function loadNews() {
  try { S.news = (await invoke('news')) || []; } catch (e) { S.news = []; }
  if (S.view === 'home') render(true);
}
const notesLine = (txt) => String(txt || '').replace(/\r/g, '').split('\n').filter((l) => !/^\s*#/.test(l)).map((l) => l.replace(/^[#>*\-\s]+/, '').replace(/[*_`]/g, '').replace(/\[([^\]]+)\]\([^)]*\)/g, '$1').trim()).filter((l) => l.length > 3).slice(0, 2).join(' ');
function newsSection() {
  const n = S.news || [];
  if (!n.length) return '';
  const list = n.length === 1 ? n : n.slice(0, Math.min(4, n.length - (n.length % 2)));
  return `<section class="shelf"><div class="shelf-h"><div><h2>${esc(t('What\'s new for your games'))}</h2></div></div><div class="news-grid${list.length === 1 ? ' one' : ''}">${list.map((x) => { const a = byId(x.id) || {}; return `<button type="button" class="news" data-act="news-open" data-id="${esc(x.id)}">
      ${a.cover ? pic(a.cover, '', 'loading="lazy"') : '<span class="ph"></span>'}
      <span class="in"><span class="kick">${esc(x.name)}</span><b>${esc(x.title)}</b><small>${esc(date(x.date))}${x.have ? ' · ' + esc(t('Installed')) : S.updates[x.id] === x.tag ? ' · ' + esc(t('Ready to update')) : ''}</small><span class="p">${esc(notesLine(x.notes) || t('No notes for this one.'))}</span></span></button>`; }).join('')}</div></section>`;
}

// ---------- pages ----------
function offlineNote() {
  return S.offline ? `<div class="offline"><span style="width:8px;height:8px;border-radius:50%;background:var(--warn);flex:none"></span>${esc(t('You\'re offline, so this is the list as it was the last time the launcher could reach it.'))}</div>` : '';
}

function pageHome() {
  const apps = visible();
  if (!apps.length) return S.cat ? `<div class="page">${offlineNote()}<div class="empty"><h2>${esc(t('Nothing to discover right now'))}</h2></div></div>` : `<div class="loading">${esc(t('Loading…'))}</div>`;
  const mine = apps.filter(forOS);
  const pool = (mine.length ? mine : apps).filter((a) => !a.soon);
  const feats = bannerApps() || oneEach(uniq([...pool.filter((a) => a.featured && (a.hero || a.cover)), ...[...pool].filter((a) => a.hero).sort(byDl), ...[...pool].sort(byDl)].map((a) => a.id)).map(byId)).slice(0, 5);
  S.slide = Math.min(S.slide || 0, Math.max(0, feats.length - 1));
  const played = Object.values(S.library).sort((x, y) => (y.last_played || 0) - (x.last_played || 0) || (y.installed_at || 0) - (x.installed_at || 0)).map((g) => libApp(g));
  const found = oneEach(Object.keys(S.owned).filter((id) => !S.library[id]).map(byId).filter((a) => a && !a.soon && forOS(a) && !(S.settings.hide_ai && isAI(a)) && !haveAny(a)).sort(byDl));
  const easy = oneEach(pool.filter((a) => S.easy.has(a.id) && !haveAny(a)));
  const fav = favs().map((id) => libApp(S.library[id]));
  return `<div class="page home">${offlineNote()}
    ${feats.length ? slider(feats) : ''}
    ${played.length ? shelf('played', t('Jump back in'), played, { mine: true, quick: true, see: 'library', sub: (a) => esc(S.library[a.id] && S.library[a.id].last_played ? tf('Played {when}', { when: ago(S.library[a.id].last_played) }) : t('Not played yet')) }) : ''}
    ${fav.length ? shelf('favs', t('Favorites'), fav, { mine: true, quick: true }) : ''}
    ${shelf('updated', t('Recently updated'), recentlyUpdated(pool), { sub: (a) => esc(tf('Updated {when}', { when: ago(Date.parse(a.released) / 1000) })) })}
    ${follows().length ? shelf('follow', t('From developers you follow'), oneEach(pool.filter((a) => a.dev && follows().includes(a.dev)).sort(byNew), true), { sub: (a) => esc((devOf(a) || {}).name || a.author || '') }) : ''}
    ${found.length ? foundSection(found) : (S.settings.rom_dirs || []).length ? '' : foundCallout()}
    ${shelf('popular', t('Popular right now'), oneEach([...pool].sort(byDl)), { see: 'popular' })}
    ${shelf('new', t('Just added'), oneEach([...pool].sort(byNew), true), { see: 'new' })}
    ${easy.length >= 2 ? shelf('easy', t('Drop in your disc'), easy, { note: t('Pick your disc image and these are set up for you, with their settings right in the launcher.') }) : ''}
    ${newsSection()}
  </div>`;
}

// Games the player owns (found in their ROM folders) that have a recomp they haven't installed.
function foundCallout() {
  return `<div class="callout"><span class="ic">${DISC_L}</span><div class="k"><b>${esc(t('It finds the games you already have'))}</b><span>${esc(t('Point the launcher at your ROM and disc image folders. It shows which have recomps and sets them up with your copy.'))}</span></div><button type="button" class="btn main" data-act="rom-add">${esc(t('Add a folder'))}</button></div>`;
}
// Games the player already has, found in their folders: one click to install and set up.
function foundSection(found) {
  const list = found.length === 1 ? found : found.slice(0, Math.min(4, found.length - (found.length % 2)));
  const more = found.length - list.length;
  return `<section class="shelf"><div class="shelf-h"><div><h2>${esc(t('Found in your folders'))}</h2><p>${esc(found.length === 1 ? t('You already have this game. Install one and the launcher sets it up with your copy.') : t('You already have these games. Install one and the launcher sets it up with your copy.'))}${more ? ' ' + esc(tn(more, '1 more in Discover.', '{n} more in Discover.')) : ''}</p></div></div>
    <div class="news-grid${list.length === 1 ? ' one' : ''}">${list.map((a) => `<div class="news found-card"><button type="button" class="fc-open" data-act="open" data-id="${esc(a.id)}" aria-label="${esc(a.name)}">${a.cover ? pic(a.cover, '', 'loading="lazy"') : '<span class="ph"></span>'}
      <span class="in"><span class="kick">${esc(t('You have it'))}</span><b>${esc(a.name)}</b><small class="based">${DISC_S}${esc(baseName(S.owned[a.id].path))}</small><span class="p">${esc([a.game, originName(a.origin)].filter(Boolean).join(' · '))}</span></span></button>
      <div class="acts">${S.jobs[a.id] ? progress(a.id) : `<button type="button" class="btn main" data-act="install" data-id="${esc(a.id)}">${DOWN}${esc(t('Install'))}</button>`}</div></div>`).join('')}</div></section>`;
}
// The games Ruben picks for the top of Home (the catalog's "banner"), in his order. Null when the
// catalog has none, or none of them can be shown here, and Home picks them itself as before.
window.bannerApps = () => {
  const ids = S.cat && Array.isArray(S.cat.banner) ? S.cat.banner : [];
  if (!ids.length) return null;
  const vis = new Set(visible().map((a) => a.id));
  const list = uniq(ids).map(byId).filter((a) => a && vis.has(a.id) && !a.soon && forOS(a) && (a.hero || a.cover));
  return list.length ? list : null;
};
// Ruben's own tag and line for a game on the banner, if he gave it one.
window.bannerInfo = (id) => (S.cat && S.cat.bannerInfo && typeof S.cat.bannerInfo === 'object' && S.cat.bannerInfo[id]) || {};
// A logo that didn't load stays a written title, also when the page is drawn again.
const BAD_LOGOS = new Set();
window.logoFail = (img) => {
  const src = img.getAttribute('src');
  BAD_LOGOS.add(src);
  // The page may have been redrawn while it loaded: mark the one on screen too.
  for (const el of [img, ...$$('.sl-logo img')]) if (el.getAttribute('src') === src && el.parentNode) el.parentNode.classList.add('fail');
};
function slideTitle(a) {
  const src = a.logo ? media(a.logo) : '';
  if (!src || BAD_LOGOS.has(src)) return `<h2>${esc(a.name)}</h2>`;
  return `<div class="sl-logo"><img src="${esc(src)}" alt=""><h2>${esc(a.name)}</h2></div>`;
}
function slider(feats) {
  return `<section class="slider" id="slider" aria-roledescription="carousel" aria-label="${esc(t('Featured'))}">${feats.map((a, i) => `<div class="slide${i === S.slide ? ' on' : ''}" data-i="${i}" aria-hidden="${i !== S.slide}">${pic(a.hero || a.cover, 'bg')}
      <div class="sl-in"><span class="kick">${esc(bannerInfo(a.id).tag || (a.featured ? t('Featured') : t('Popular right now')))}${a.origin ? ` · ${esc(originName(a.origin))}` : ''}</span>${slideTitle(a)}${based(a)}${bannerInfo(a.id).text || a.blurb ? `<p>${esc(bannerInfo(a.id).text || a.blurb)}</p>` : ''}
        <div class="acts">${actions(a, 'big')}<button type="button" class="btn big glass" data-act="open" data-id="${esc(a.id)}">${esc(t('Details'))}</button></div></div>
      <div class="sl-cover">${pic(a.cover)}</div></div>`).join('')}
    ${feats.length > 1 ? `<div class="sl-ui">${feats.map((a, i) => `<button type="button" class="dot${i === S.slide ? ' on' : ''}" data-act="slide" data-i="${i}" aria-label="${esc(tf('Show {name}', { name: a.name }))}"></button>`).join('')}</div>
    <div class="sl-arrows"><button type="button" data-act="slide-step" data-d="-1" aria-label="${esc(t('Previous'))}">${CHEV_L}</button><button type="button" data-act="slide-step" data-d="1" aria-label="${esc(t('Next'))}">${CHEV_R}</button></div>` : ''}</section>`;
}
function showSlide(i) {
  const s = $('#slider');
  if (!s) return;
  const n = $$('.slide', s).length;
  if (!n) return;
  S.slide = ((i % n) + n) % n;
  $$('.slide', s).forEach((x, k) => { x.classList.toggle('on', k === S.slide); x.setAttribute('aria-hidden', String(k !== S.slide)); });
  $$('.sl-ui .dot', s).forEach((x, k) => x.classList.toggle('on', k === S.slide));
}
setInterval(() => {
  const s = $('#slider');
  if (!s || document.hidden || S.away || s.matches(':hover') || document.body.classList.contains('tv-on') || $('.scrim')) return;
  if (document.body.classList.contains('lighter') || S.running.size) return;
  showSlide((S.slide || 0) + 1);
}, 8000);
function shelf(key, title, list, o = {}) {
  if (!list.length) return '';
  return `<section class="shelf" data-shelf="${key}" data-n="${Math.min(24, list.length)}"><div class="shelf-h"><div><h2>${esc(title)}</h2>${o.note ? `<p>${esc(o.note)}</p>` : ''}</div>
    <div class="pager"><small></small>${o.see ? `<button type="button" class="see" ${o.see === 'library' ? 'data-go="library"' : `data-act="see-all" data-sort="${o.see}"`}>${esc(t('See all'))}</button>` : ''}<button type="button" class="ar" data-act="shelf" data-d="-1" aria-label="${esc(t('Back'))}">${CHEV_L}</button><button type="button" class="ar" data-act="shelf" data-d="1" aria-label="${esc(t('More'))}">${CHEV_R}</button></div></div>
    <div class="viewport"><div class="track">${list.slice(0, 24).map((a) => tile(a, { mine: o.mine, quick: o.quick, sub: o.sub ? o.sub(a) : undefined })).join('')}</div></div></section>`;
}
// Each shelf shows as many games as fit, and pages through the rest.
function layoutShelves(scope = document) {
  for (const sh of $$('.shelf[data-shelf]', scope)) {
    const vp = $('.viewport', sh), tr = $('.track', sh);
    if (!vp || !tr) continue;
    const w = tr.clientWidth, n = Number(sh.dataset.n) || 0, gap = 20;
    if (!w) continue;
    const per = Math.max(2, Math.min(10, Math.floor((w + gap) / (150 + gap))));
    const pages = Math.max(1, Math.ceil(n / per));
    const key = sh.dataset.shelf;
    const p = Math.min(Math.max(0, S.shelf[key] || 0), pages - 1);
    S.shelf[key] = p;
    sh.style.setProperty('--per', per);
    tr.style.transform = `translateX(${-p * (w + gap)}px)`;
    const [prev, next] = $$('.ar', sh);
    prev.disabled = p === 0; next.disabled = p >= pages - 1;
    prev.hidden = next.hidden = pages < 2;
    $('.pager small', sh).textContent = pages > 1 ? tf('{page} of {pages}', { page: p + 1, pages }) : '';
    $$('.tile', tr).forEach((el, i) => { const vis = i >= p * per && i < (p + 1) * per; el.inert = !vis; });
  }
}

function discoverList() {
  const f = S.find, q = S.q.trim().toLowerCase();
  let list = visible().filter((a) => (!f.genre || a.genre === f.genre) && (!f.origin || a.origin === f.origin) && (!f.tag || tagsOf(a).includes(f.tag)) && showOk(a, f.show) && forOS(a) && (!f.great || playsGreat(a)) && (!f.easy || (S.easy.has(a.id) && !a.soon)) && (!f.mp || isMulti(a))
    && (!q || [a.name, a.game, a.author, originName(a.origin), GENRES[a.genre], t(GENRES[a.genre]), ...tagsOf(a)].join(' ').toLowerCase().includes(q)));
  const soonLast = (x, y) => (x.soon ? 1 : 0) - (y.soon ? 1 : 0);
  if (f.sort === 'new') list.sort(byNew); else if (f.sort === 'updated') list.sort(byUpdated); else if (f.sort === 'az') list.sort((x, y) => x.name.localeCompare(y.name)); else if (f.sort === 'stars') list.sort((x, y) => (y.stars || 0) - (x.stars || 0)); else list.sort(byDl);
  return oneEach(list.sort(soonLast), f.sort === 'new');
}

function pageDiscover() {
  if (!S.cat) return `<div class="loading">${esc(t('Loading…'))}</div>`;
  const f = S.find, apps = visible();
  const genres = Object.keys(GENRES).filter((k) => apps.some((a) => a.genre === k));
  const q = S.q.trim();
  return `<div class="page">${offlineNote()}
    <div class="dhead"><h1>${esc(q ? t('Search') : f.tag ? tf('All recomps with {tag}', { tag: f.tag }) : f.genre ? t(GENRES[f.genre]) : t('Discover'))}</h1><p>${esc(t('Every recomp comes straight from its developer\'s GitHub. You bring your own copy of the game.'))}</p>
      <label class="bigsearch"><span class="sr">${esc(t('Search recomps'))}</span>${SEARCH}<input id="dq" type="search" placeholder="${esc(t('Search games, recomps or developers'))}" value="${esc(S.q)}" autocomplete="off" spellcheck="false"><button type="button" class="clear" data-act="clear-q" aria-label="${esc(t('Clear the search'))}"${q ? '' : ' hidden'}>${XMARK}</button></label></div>
    <div class="dctl rows"><div class="grp">
      <div class="seg big" role="radiogroup" aria-label="${esc(t('Sort'))}">${[['popular', t('Popular')], ['new', t('Newest')], ['updated', t('Recently updated')], ['stars', t('Most stars')], ['az', t('A to Z')]].map(([k, n]) => `<button type="button" role="radio" aria-checked="${f.sort === k}" data-act="sort" data-k="${k}">${esc(n)}</button>`).join('')}</div>
    </div><div class="grp">
      <label class="pill-sel">${esc(t('Console'))}<select class="sel" data-act="origin">${[['', t('Any')], ...(S.cat.origins || []).map((o) => [o.id, o.name])].map(([k, n]) => `<option value="${esc(k)}"${f.origin === k ? ' selected' : ''}>${esc(n)}</option>`).join('')}</select></label>
      <label class="pill-sel">${esc(t('Genre'))}<select class="sel" data-act="genre-sel">${[['', t('Any')], ...genres.map((k) => [k, t(GENRES[k])])].map(([k, n]) => `<option value="${esc(k)}"${f.genre === k ? ' selected' : ''}>${esc(n)}</option>`).join('')}</select></label>
      <label class="pill-sel">${esc(t('Show'))}<select class="sel" data-act="show-sel">${SHOWS.map(([k, n]) => `<option value="${k}"${(f.show || '') === k ? ' selected' : ''}>${esc(t(n))}</option>`).join('')}</select></label>
      <label class="pill-sel">${esc(t('Tag'))}<select class="sel" data-act="tag-sel">${[['', t('Any')], ...allTags().map((k) => [k, k])].map(([k, n]) => `<option value="${esc(k)}"${(f.tag || '') === k ? ' selected' : ''}>${esc(n)}</option>`).join('')}</select></label>
    </div><div class="grp">
      <button type="button" class="toggle" role="switch" aria-checked="${!!f.easy}" data-act="easy-f">${DISC_S}${esc(t('Easy Setup'))}<span class="switch" aria-hidden="true"></span></button>
      <button type="button" class="toggle" role="switch" aria-checked="${!!f.mp}" data-act="mp-f">${esc(t('Multiplayer'))}<span class="switch" aria-hidden="true"></span></button>
      <button type="button" class="toggle" role="switch" aria-checked="${!!f.great}" data-act="great"><span class="dot-great" aria-hidden="true"></span>${esc(t('Plays great'))}<span class="switch" aria-hidden="true"></span></button>
      <button type="button" class="toggle" role="switch" aria-checked="${!!S.settings.hide_ai}" data-act="toggle" data-k="hide_ai">${esc(t('Hide AI-assisted'))}<span class="switch" aria-hidden="true"></span></button>
    </div></div>
    ${f.tag ? `<div class="chips tagchips"><button type="button" class="chip on" data-act="clear-tag" aria-label="${esc(t('Clear the search'))}">${esc(t('Tag'))}: ${esc(f.tag)} ${XMARK}</button></div>` : ''}
    <div id="dlist">${discoverGrid()}</div>
  </div>`;
}
// Every tag in the list, the most used first.
function allTags() {
  const n = {};
  for (const a of visible()) for (const tg of tagsOf(a)) n[tg] = (n[tg] || 0) + 1;
  return Object.keys(n).sort((x, y) => n[y] - n[x] || x.localeCompare(y));
}
function discoverGrid() {
  const f = S.find, list = discoverList();
  return list.length ? `<div class="count-line">${esc(tn(list.length, '1 recomp', '{n} recomps'))}</div><div class="grid" style="margin-top:20px">${list.map((a) => tile(a, { quick: true })).join('')}</div>`
    : `<div class="empty"><h2>${esc(t('Nothing here'))}</h2><p>${esc(f.great ? t('Nothing here has enough player reports yet. Turn off “Plays great” to see the rest.') : f.easy ? t('No Easy Setup recomps match. Turn off “Easy Setup” to see the rest.') : f.mp ? t('No multiplayer recomps match. Turn off “Multiplayer” to see the rest.') : t('Try another search, console or genre.'))}</p></div>`;
}

// A Library filter whose games are all gone (the last update installed, the last copy set up,
// the last favorite removed): back to All, since its chip isn't there to switch back with.
function libFilterOk() {
  const f = S.lib.filter;
  const any = f === 'update' ? Object.keys(S.updates).some((id) => S.library[id])
    : f === 'setup' ? Object.values(S.library).some((g) => S.easy.has(g.id) && !g.game_copy)
      : f === 'fav' ? favs().length > 0 : f === 'saved' || f === 'inst' ? savedItems().length > 0 : true;
  if (!any) S.lib.filter = 'all';
  if (S.lib.origin && !Object.values(S.library).some((g) => (byId(g.id) || {}).origin === S.lib.origin)) S.lib.origin = '';
}
// Added but not installed yet: in the Library like the rest, with Install on their page.
function savedItems() {
  return Object.entries(saved()).filter(([id]) => !S.library[id] && byId(id)).map(([id, when]) => ({ g: { id, name: byId(id).name, installed_at: when }, a: byId(id), saved: true }));
}
function libList() {
  libFilterOk();
  const q = S.lq.trim().toLowerCase();
  let list = Object.values(S.library).map((g) => ({ g, a: libApp(g) })).concat(savedItems());
  if (S.lib.filter === 'saved') list = list.filter((x) => x.saved);
  if (S.lib.filter === 'inst') list = list.filter((x) => !x.saved);
  if (S.lib.filter === 'update') list = list.filter((x) => S.updates[x.g.id]);
  if (S.lib.filter === 'setup') list = list.filter((x) => S.easy.has(x.g.id) && !x.g.game_copy);
  if (S.lib.filter === 'fav') list = list.filter((x) => favs().includes(x.g.id));
  if (S.lib.origin) list = list.filter((x) => x.a.origin === S.lib.origin);
  if (q) list = list.filter((x) => [x.a.name, x.a.game].join(' ').toLowerCase().includes(q));
  if (S.lib.sort === 'az') list.sort((x, y) => x.a.name.localeCompare(y.a.name));
  else if (S.lib.sort === 'time') list.sort((x, y) => (y.g.played_secs || 0) - (x.g.played_secs || 0));
  else list.sort((x, y) => (y.g.last_played || 0) - (x.g.last_played || 0) || (y.g.updated_at || y.g.installed_at || 0) - (x.g.updated_at || x.g.installed_at || 0));
  // More than one version of a game installed: one tile, the one played last, with how many.
  const T = index().titles, seen = new Map(), out = [];
  for (const x of list) {
    const k = x.a.title && T.has(x.a.title) ? x.a.title : null;
    if (!k) { out.push(x); continue; }
    if (seen.has(k)) { seen.get(k).n++; continue; }
    const lead = list.filter((y) => y.a.title === k).sort((p, q) => (q.g.last_played || 0) - (p.g.last_played || 0))[0];
    const item = Object.assign({}, lead, { n: 1 });
    seen.set(k, item); out.push(item);
  }
  return out;
}

function pageLibrary() {
  libFilterOk();
  const nsaved = savedItems().length;
  const total = Object.keys(S.library).length + nsaved;
  if (!total) {
    return `<div class="page"><div class="head"><div><h1>${esc(t('Library'))}</h1><p>${esc(t('Your recomps, kept up to date from each developer\'s own GitHub.'))}</p></div></div>
      <div class="empty"><svg width="44" height="44" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="4" y="4" width="6.5" height="6.5" rx="1.5"/><rect x="13.5" y="4" width="6.5" height="6.5" rx="1.5"/><rect x="4" y="13.5" width="6.5" height="6.5" rx="1.5"/><rect x="13.5" y="13.5" width="6.5" height="6.5" rx="1.5"/></svg>
      <h2>${esc(t('Nothing installed yet'))}</h2><p>${esc(t('Pick a recomp in Discover and install it. It shows up here, ready to play.'))}</p><div class="acts"><button type="button" class="btn" data-act="adopt">${esc(t('Bring Your Recomps'))}</button><button type="button" class="btn main" data-go="discover">${esc(t('Discover recomps'))}</button>${devOn() ? `<button type="button" class="btn" data-act="dev-add">${esc(t('Add a local build'))}</button>` : ''}</div></div></div>`;
  }
  const n = Object.keys(S.updates).filter((id) => S.library[id]).length;
  const setup = Object.values(S.library).filter((g) => S.easy.has(g.id) && !g.game_copy).length;
  const nf = favs().length;
  // Consoles the player's games are from: a filter once there's more than one.
  const origins = [...new Set(Object.values(S.library).map((g) => (byId(g.id) || {}).origin).filter(Boolean))];
  const consoleSel = origins.length > 1 ? `<label class="pill-sel">${esc(t('Console'))}<select class="sel" data-act="liborigin">${[['', t('Any')], ...origins.map((o) => [o, originName(o)])].map(([k, nm]) => `<option value="${esc(k)}"${(S.lib.origin || '') === k ? ' selected' : ''}>${esc(nm)}</option>`).join('')}</select></label>` : '';
  return `<div class="page"><div class="head"><div><h1>${esc(t('Library'))}</h1><p>${esc(tn(total, '1 game. Right-click one for versions, launch options and more.', '{n} games. Right-click one for versions, launch options and more.'))}</p></div>
      <div class="dctl"><label class="smallsearch"><span class="sr">${esc(t('Search your games'))}</span>${SEARCH.replace('width="20" height="20"', 'width="16" height="16"')}<input id="lq" type="search" placeholder="${esc(t('Search your games'))}" value="${esc(S.lq)}" autocomplete="off" spellcheck="false"></label>
      <button type="button" class="btn" data-act="adopt">${esc(t('Bring Your Recomps'))}</button>
      <button type="button" class="btn" data-act="gh-add">${GITHUB_S}${esc(t('Add From GitHub'))}</button>
      ${devOn() ? `<button type="button" class="btn" data-act="dev-add">${esc(t('Add a local build'))}</button>` : ''}
      ${consoleSel}<label class="pill-sel">${esc(t('Sort'))}<select class="sel" data-act="libsort"><option value="played"${S.lib.sort === 'played' ? ' selected' : ''}>${esc(t('Recently played'))}</option><option value="time"${S.lib.sort === 'time' ? ' selected' : ''}>${esc(t('Most played'))}</option><option value="az"${S.lib.sort === 'az' ? ' selected' : ''}>${esc(t('A to Z'))}</option></select></label></div></div>
    ${n || setup || nf || nsaved ? `<div class="chips"><button type="button" class="chip${S.lib.filter === 'all' ? ' on' : ''}" data-act="libf" data-f="all">${esc(t('All'))} <b>${fmtNum(total)}</b></button>${nsaved ? `<button type="button" class="chip${S.lib.filter === 'inst' ? ' on' : ''}" data-act="libf" data-f="inst">${esc(t('Installed'))} <b>${fmtNum(Object.keys(S.library).length)}</b></button>` : ''}${n ? `<button type="button" class="chip${S.lib.filter === 'update' ? ' on' : ''}" data-act="libf" data-f="update">${esc(t('Update ready'))} <b>${fmtNum(n)}</b></button>` : ''}${setup ? `<button type="button" class="chip${S.lib.filter === 'setup' ? ' on' : ''}" data-act="libf" data-f="setup">${esc(t('Needs your copy'))} <b>${fmtNum(setup)}</b></button>` : ''}${nf ? `<button type="button" class="chip${S.lib.filter === 'fav' ? ' on' : ''}" data-act="libf" data-f="fav">${STAR}${esc(t('Favorites'))} <b>${fmtNum(nf)}</b></button>` : ''}${nsaved ? `<button type="button" class="chip${S.lib.filter === 'saved' ? ' on' : ''}" data-act="libf" data-f="saved">${esc(t('Not Installed'))} <b>${fmtNum(nsaved)}</b></button>` : ''}${n > 1 ? `<button type="button" class="chip main" data-act="update-all">${esc(tf('Update All ({n})', { n }))}</button>` : ''}</div>` : ''}
    <div id="llist" class="${libWide() ? 'wide' : ''}" style="--tile:${libTile()}">${libraryGrid()}</div>
    ${(S.settings.ui || {}).hide_tile_ctl ? '' : `<div class="tile-size"><svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><rect x="5" y="5" width="6" height="6" rx="1"/><rect x="13" y="13" width="6" height="6" rx="1"/></svg><input type="range" min="120" max="360" step="10" value="${libTile()}" data-act="tile-size" aria-label="${esc(t('Tile size'))}" title="${esc(t('Tile size'))}"><button type="button" class="ts-view" data-act="libview" aria-label="${esc(libWide() ? t('Show covers') : t('Show wide tiles'))}" title="${esc(libWide() ? t('Show covers') : t('Show wide tiles'))}">${libWide() ? VIEW_WIDE : VIEW_COVERS}</button></div>`}</div>`;
}
// How wide the Library's covers are, in pixels (the slider at the bottom right).
const libTile = () => Math.max(120, Math.min(360, Number((S.settings.ui || {}).tile) || 168));
// The Library as covers, or as wide tiles: the game's art, blurred and dark, with its logo on it.
const libWide = () => (S.settings.ui || {}).lib_view === 'wide';
const VIEW_COVERS = '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><rect x="3.5" y="3.5" width="7" height="10" rx="1.5"/><rect x="13.5" y="3.5" width="7" height="10" rx="1.5"/><path d="M3.5 18h7M13.5 18h7"/></svg>';
const VIEW_WIDE = '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><rect x="3" y="4" width="18" height="7" rx="1.5"/><rect x="3" y="14" width="18" height="7" rx="1.5"/></svg>';
// The Library's tiles glide to their new places and sizes when the size or view changes, instead
// of jumping (only the ones you can see; the rest just move).
function flipGrid(box, change) {
  const H = innerHeight;
  const items = $$('.tile', box).filter((el) => { const r = el.getBoundingClientRect(); return r.bottom > -200 && r.top < H + 200; }).slice(0, 80);
  const before = new Map(items.map((el) => [el, el.getBoundingClientRect()]));
  items.forEach((el) => el.getAnimations().forEach((x) => x.cancel()));
  change();
  if (matchMedia('(prefers-reduced-motion: reduce)').matches) return;
  for (const el of items) {
    const a = before.get(el), b = el.getBoundingClientRect();
    if (!a || !b.width) continue;
    const dx = a.left - b.left, dy = a.top - b.top, k = a.width / b.width;
    if (Math.abs(dx) < 1 && Math.abs(dy) < 1 && Math.abs(k - 1) < 0.01) continue;
    el.animate([{ transformOrigin: '0 0', transform: `translate(${dx.toFixed(1)}px,${dy.toFixed(1)}px) scale(${k.toFixed(4)})` }, { transformOrigin: '0 0', transform: 'none' }], { duration: 240, easing: 'cubic-bezier(.2,.8,.2,1)' });
  }
}
function libraryGrid() {
  const list = libList();
  const wide = libWide();
  return list.length ? `<div class="grid compact">${list.map((x) => x.saved ? tile(x.a, { wide, sub: esc(t('Not installed')) }) : tile(x.a, { wide, mine: true, quick: true, inst: x.n, sub: esc(S.running.has(x.g.id) ? t('Playing now') : x.g.last_played ? tf('Played {when}', { when: ago(x.g.last_played) }) : t('Not played yet')) })).join('')}</div>` : `<div class="empty"><p>${esc(t('Nothing matches.'))}</p></div>`;
}




function pageGame(id) {
  let a = byId(id);
  if (!a && S.library[id]) a = { id, name: S.library[id].name, repo: S.library[id].repo, systems: [S.os] };
  if (!a) return S.cat ? `<div class="page"><div class="empty"><h2>${esc(t('This recomp isn\'t listed anymore'))}</h2><button type="button" class="btn" data-go="discover">${esc(t('Back to Discover'))}</button></div></div>` : `<div class="loading">${esc(t('Loading…'))}</div>`;
  const inst = S.library[id];
  if (S.mods[id] === undefined) loadMods(id);
  if (inst && S.setup[id] === undefined) loadSetup(id);
  const su = S.setup[id];
  const chips = [isAI(a) ? `<span class="ai-tag">${esc(t('AI-assisted'))}</span>` : '', inst && inst.local ? `<span class="tag cy">${esc(inst.tag === 'own' ? t('Added by You') : t('Local build'))}</span>` : '', S.easy.has(id) ? `<span class="tag cy">${DISC_S}${esc(t('Easy Setup'))}</span>` : '', su && su.copy ? `<span class="tag">${esc(t('Your copy is set up'))}</span>` : '', lookChip(inst)].join('');
  const kick = [GENRES[a.genre] ? t(GENRES[a.genre]) : '', originName(a.origin)].filter(Boolean).join(' · ');
  return `<div class="gp${a.hero || a.cover ? '' : ' noart'}" data-gp="${esc(id)}">
    <div class="gp-amb">${pic(a.cover)}</div>
    <div class="gp-hero">${pic(a.hero || a.cover)}<button type="button" class="btn glass gp-back" data-act="back">${CHEV_L}${esc(t('Back'))}</button></div>
    <div class="gp-top"><div class="gp-cover">${pic(a.cover)}</div>
      <div class="gp-title">${kick ? `<span class="kick">${esc(kick)}</span>` : ''}<h1>${esc(a.name)}</h1>${based(a)}${devLine(a)}${verPill(a)}${chips ? `<div class="gp-chips">${chips}</div>` : ''}${tagsOf(a).length ? `<div class="gp-tags" aria-label="${esc(t('Tags'))}">${tagsOf(a).map((tg) => `<button type="button" class="tagpill" data-act="tag" data-tag="${esc(tg)}" title="${esc(tf('All recomps with {tag}', { tag: tg }))}">${esc(tg)}</button>`).join('')}</div>` : ''}</div>
      <div class="gp-acts" data-acts="${esc(id)}">${actions(a)}</div></div>
    <div class="tabs"><div class="tabs-in" role="tablist"><span class="tab-pill" aria-hidden="true"></span>${gameTabs(a).map(([k, n, x]) => `<button type="button" role="tab" class="${S.tab === k ? 'on' : ''}" aria-selected="${S.tab === k}" data-act="tab" data-tab="${k}">${esc(n)}${x || ''}</button>`).join('')}</div></div>
    <div class="g-body" id="g-body">${gameBody(a)}</div></div>`;
}
// RecompLauncher itself in Steam, starting in Big Picture mode (Gaming Mode on a Steam Deck).
window.selfToSteam = (r) => selfToSteam(r);
async function selfToSteam(restart) {
  let r;
  try { r = await invoke('launcher_to_steam', { restart }); } catch (e) { return fail(e); }
  if (r === 'steam_running') {
    const v = await (window.ask || ask)(t('Close Steam for a moment?'), `<p>${esc(t('Steam only reads its list of games when it starts. It closes, RecompLauncher is added, and Steam opens again.'))}</p>`,
      [{ label: t('Not now'), v: null }, { label: t('Close and add'), v: 'yes', main: true }]);
    if (v === 'yes') return selfToSteam(true);
    return;
  }
  await saveUi({ in_steam: true }, true);
  toast(t('RecompLauncher is in your Steam library. It opens in Big Picture mode from there.'));
  if (S.view === 'settings') render(true);
}

// ---------- versions of the same game ----------
const OS_SHORT = (a) => (a.systems || []).filter((k) => OS_NAME[k]);
function verRow(v, cur, g, cls) {
  const inst = S.library[v.id], here = v.id === cur;
  const tags = [v.id === g.pick ? `<span class="vtag rec">${esc(t('Recommended'))}</span>` : '', inst ? `<span class="vtag inst">${esc(S.running.has(v.id) ? t('Playing') : t('Installed'))}</span>` : '', v.soon ? `<span class="vtag">${esc(t('Soon'))}</span>` : ''].join('');
  const line = [esc((devOf(v) || {}).name || v.author || '') + (devOf(v) && devOf(v).verified ? TICK(12) : ''), esc(v.adds || '')].filter(Boolean).join(' · ');
  const os = OS_SHORT(v);
  return `<button type="button" class="${cls}${here ? ' on' : ''}" ${cls === 'vm-row' ? 'role="option" ' : ''}aria-${cls === 'vm-row' ? 'selected' : 'current'}="${here}" data-act="ver-go" data-id="${esc(v.id)}">
    <span class="v-ic">${v.icon ? pic(v.icon, '', 'loading="lazy"') : v.cover ? pic(v.cover, '', 'loading="lazy"') : ''}</span>
    <span class="v-txt"><span class="v-n"><b>${esc(v.name)}</b>${runScore(v.id)}${tags}</span>${line ? `<small>${line}</small>` : ''}</span>
    <span class="v-os">${os.map((k) => `<span title="${esc(OS_NAME[k])}">${osIcon(k)}</span>`).join('')}</span>
    <span class="v-dl"><b>${esc(v.downloads ? fmtN(v.downloads) : '—')}</b><small>${esc(t('downloads'))}</small></span>
    <span class="v-go">${here ? `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m5 12.5 4.5 4.5L19 7.5"/></svg>` : CHEV_R}</span></button>`;
}
// Under the title: which version this is, and a menu to switch.
function verPill(a) {
  const g = verGroup(a);
  if (!g) return '';
  const n = g.list.filter((v) => S.library[v.id]).length;
  return `<button type="button" class="ver-pill" data-act="ver-menu" data-id="${esc(a.id)}" aria-haspopup="listbox" aria-expanded="false">${LAYERS}<span>${esc(tn(g.list.length, '1 version', '{n} versions'))}</span>${n > 1 ? `<em>${esc(tn(n, '1 installed', '{n} installed'))}</em>` : ''}<svg class="chev" width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><path d="m2.5 4.5 3.5 3.5 3.5-3.5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/></svg></button>`;
}
// On Overview: every version, like the website's list.
function versionsCard(a) {
  const g = verGroup(a);
  if (!g) return '';
  const hide = !!(S.settings.ui || {}).verHide;
  return `<div class="card vcard${hide ? ' shut' : ''}"><div class="vc-h"><span class="vc-stack">${g.list.slice(0, 4).map((v) => `<i>${v.icon || v.cover ? pic(v.icon || v.cover, '', 'loading="lazy"') : ''}</i>`).join('')}</span>
      <div><h3>${esc(tf('{n} versions of {game}', { n: g.list.length, game: g.name }))}</h3><span>${esc(tf('You\'re on {name}', { name: a.name }))}</span></div>
      <button type="button" class="btn sm" data-act="ver-hide" aria-expanded="${!hide}">${esc(hide ? t('Show') : t('Hide'))}</button></div>
    ${hide ? '' : `<div class="vc-list">${g.list.map((v) => verRow(v, a.id, g, 'vc-row')).join('')}</div>
    <p class="vc-note">${esc(t('Each version installs on its own, so you can keep more than one and try them side by side.'))}</p>`}</div>`;
}
function verMenu(el) {
  const a = byId(el.dataset.id), g = verGroup(a);
  if (!g) return;
  const wasOpen = el.getAttribute('aria-expanded') === 'true';
  closeMenu();
  if (wasOpen) { el.setAttribute('aria-expanded', 'false'); return; }
  const m = document.createElement('div');
  m.className = 'menu vermenu';
  m.dataset.for = a.id;
  m.setAttribute('role', 'listbox');
  m.setAttribute('aria-label', tf('{n} versions of {game}', { n: g.list.length, game: g.name }));
  m.innerHTML = `<div class="vm-h">${esc(tf('{n} versions of {game}', { n: g.list.length, game: g.name }))}</div>${g.list.map((v) => verRow(v, a.id, g, 'vm-row')).join('')}`;
  document.body.append(m);
  const r = el.getBoundingClientRect();
  const w = Math.min(560, innerWidth - 24);
  m.style.width = w + 'px';
  m.style.left = Math.max(12, Math.min(r.left, innerWidth - w - 12)) + 'px';
  const below = innerHeight - r.bottom - 16, above = r.top - 16;
  if (below < 240 && above > below) { m.style.bottom = (innerHeight - r.top + 8) + 'px'; m.style.maxHeight = above + 'px'; m.classList.add('up'); }
  else { m.style.top = (r.bottom + 8) + 'px'; m.style.maxHeight = below + 'px'; }
  el.setAttribute('aria-expanded', 'true');
  new MutationObserver((x, o) => { if (!m.isConnected) { o.disconnect(); el.setAttribute('aria-expanded', 'false'); } }).observe(document.body, { childList: true });
  const cur = $('.vm-row.on', m) || $('.vm-row', m);
  if (cur) { cur.scrollIntoView({ block: 'nearest' }); cur.focus({ preventScroll: true }); }
  m.addEventListener('keydown', (e) => {
    const rows = $$('.vm-row', m), i = rows.indexOf(document.activeElement);
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') { e.preventDefault(); const n = rows[Math.max(0, Math.min(rows.length - 1, i + (e.key === 'ArrowDown' ? 1 : -1)))]; if (n) n.focus(); }
    else if (e.key === 'Tab') { closeMenu(); el.focus(); }
  });
}
// Another version of the game in place: same tab, same spot on the page, no new step back.
function switchVersion(id) {
  closeMenu();
  if (!byId(id) || id === S.arg) return;
  const top = $('#main').scrollTop;
  const swap = () => {
    S.arg = id;
    S.anim = 'enter-soft';
    render();
    $('#main').scrollTop = top;
  };
  if (S.os === 'windows' && document.startViewTransition && !matchMedia('(prefers-reduced-motion: reduce)').matches) document.startViewTransition(swap);
  else swap();
}

function lookChip(g) {
  const l = g && g.look && window.Looks && Looks.defs() && Looks.defs().looks.find((x) => x.id === g.look);
  return l && looksHere() ? `<span class="tag">${esc(tf('{name} look', { name: t(l.name) }))}</span>` : '';
}
// A small "Beta" mark for features still being worked on (the launcher's own game settings).
const BETA_MARK = () => `<b class="tab-beta">${esc(t('Beta'))}</b>`;
function gameTabs(a) {
  const id = a.id, inst = S.library[id];
  const su = S.setup[id];
  const local = inst && inst.local && inst.tag !== 'own';
  const tabs = (local ? [['dev', t('Developer')], ['overview', t('Overview')]] : inst && inst.local ? [['overview', t('Overview')]] : [['overview', t('Overview')], ['updates', t('Updates')]]).concat((su && su !== 'loading') || (su === 'loading' && inst && S.easy.has(id)) ? [['settings', t('Game settings'), su && su.beta && su.settings && su.settings.length ? BETA_MARK() : '']] : [], inst && looksHere() ? [['looks', t('Looks')]] : [], S.mods[id] && S.mods[id] !== 'loading' ? [['mods', t('Mods')]] : [], inst ? [['saves', t('Saves')]] : [], inst ? [['installed', t('Manage')]] : []);
  if (!tabs.some(([k]) => k === S.tab)) S.tab = local ? 'dev' : 'overview';
  return tabs;
}
// ---------- how it runs, by the players' word ----------
const RUN_WORD = { runs: N_('Plays great'), issues: N_('Has problems'), broken: N_('Won\'t run') };
async function loadCounts(force) {
  try { S.runCounts = (await invoke('report_counts', { force: !!force })) || {}; } catch { S.runCounts = S.runCounts || {}; }
  if (S.view === 'game') refreshGame();
  else if (S.view === 'discover' && $('#dlist')) morphList($('#dlist'), discoverGrid());
  else if (S.view === 'home') render(true);
  if (window.tvRefresh) window.tvRefresh();
}
async function loadReports(id, force) {
  S.reports[id] = S.reports[id] && S.reports[id] !== 'loading' ? S.reports[id] : 'loading';
  try { S.reports[id] = await invoke('play_reports', { id, force: !!force }); } catch (e) { S.reports[id] = { error: true, counts: {}, items: [] }; }
  if (S.view === 'game' && S.arg === id && S.tab === 'overview') refreshGame();
}
// { n, runs, issues, broken, pct(k) } from counts.
function runTally(c) {
  c = c || {};
  const n = (c.runs || 0) + (c.issues || 0) + (c.broken || 0);
  return { n, runs: c.runs || 0, issues: c.issues || 0, broken: c.broken || 0, pct: (k) => (n ? Math.round((100 * (c[k] || 0)) / n) : 0) };
}
const runBar = (r) => `<div class="rbar" aria-hidden="true">${['runs', 'issues', 'broken'].map((k) => (r[k] ? `<i class="v-${k}" style="flex:${r[k]}"></i>` : '')).join('')}</div>`;
// Enough players said it plays great: five reports or more, most of them good.
const playsGreat = (a) => { const r = runTally((S.runCounts || {})[a.id]); return r.n >= 5 && r.pct('runs') >= 70; };
// The small score beside a version's name: how many say it plays great.
function runScore(id) {
  const r = runTally((S.runCounts || {})[id]);
  if (!r.n) return '';
  const p = r.pct('runs');
  return `<span class="vscore ${p >= 75 ? 'good' : p >= 45 ? 'mid' : 'low'}" title="${esc(`${tf('{pct}% plays great', { pct: p })} · ${tn(r.n, '1 report', '{n} reports')}`)}">${esc(tf('{pct}%', { pct: p }))}</span>`;
}
function runsCard(a) {
  if (a.soon || (S.library[a.id] && S.library[a.id].local)) return '';
  const rep = S.reports[a.id];
  if (rep === undefined) loadReports(a.id);
  const inst = S.library[a.id];
  const head = (sub) => `<div class="card-h"><h3>${esc(t('How it runs'))}</h3><span>${esc(sub)}</span></div>`;
  if (!rep || rep === 'loading') return `<div class="card runs">${head('')}<p class="muted" style="margin:6px 0 0">${esc(t('Loading…'))}</p></div>`;
  const r = runTally(rep.counts);
  const say = inst ? `<button type="button" class="btn" data-act="run-say" data-id="${esc(a.id)}">${esc(t('Say how it runs'))}</button>` : '';
  const all = `<button type="button" class="btn" data-act="url" data-url="${esc(SITE + '/game/' + encodeURIComponent(a.id))}">${esc(r.n > 3 ? tf('All {n} reports', { n: r.n }) : t('On recomplauncher.com'))}</button>`;
  const acts = `<div class="acts${say ? ' grid2' : ''}">${say}${all}</div>`;
  if (!r.n) return `<div class="card runs">${head(rep.error ? t('Can\'t be reached right now') : t('No reports yet'))}<p class="runs-empty">${esc(inst ? t('Be the first: after you play, say how it ran on your PC.') : t('Nobody has said how it runs yet. Players who try it can say so from the launcher.'))}</p>${say ? `<div class="acts">${say}</div>` : ''}</div>`;
  const items = (rep.items || []).slice(0, 3).map((x) => `<div class="rep"><div class="rep-top"><span class="rverdict v-${esc(x.verdict)}"><i aria-hidden="true"></i>${esc(t(RUN_WORD[x.verdict] || ''))}</span><small>${esc([x.version, x.when ? ago(Date.parse(x.when) / 1000 || x.when) : ''].filter(Boolean).join(' · '))}</small></div>
      ${x.pc ? `<p class="rep-pc">${esc([x.pc.gpu, x.pc.cpu, x.pc.ram ? tf('{n} GB', { n: x.pc.ram }) : '', x.pc.os].filter(Boolean).join(' · '))}</p>` : ''}
      ${x.note ? `<p class="rep-note" dir="auto">${esc(x.note)}</p>` : ''}</div>`).join('');
  return `<div class="card runs">${head(tn(r.n, '1 report', '{n} reports'))}
    <div class="runs-top"><b class="runs-pct">${esc(tf('{pct}%', { pct: r.pct('runs') }))}</b><span>${esc(t('say it plays great'))}</span></div>
    ${runBar(r)}
    <div class="rlegend">${['runs', 'issues', 'broken'].map((k) => `<div class="v-${k}"><i aria-hidden="true"></i><span>${esc(t(RUN_WORD[k]))}</span><b>${fmtNum(r[k])}</b></div>`).join('')}</div>
    ${items ? `<div class="reps">${items}</div>` : ''}${acts}</div>`;
}

function gameRail(a) {
  const facts = [
    [t('Latest version'), a.version ? 'v' + String(a.version).replace(/^v/i, '') : ''], [t('Size'), a.size], [t('Latest release'), date(a.released)], [t('First release'), date(a.first)],
    [t('Releases'), a.releases ? fmtNum(a.releases) : ''], [t('Downloads'), a.downloads ? fmtN(a.downloads) : ''], [t('Stars on GitHub'), a.stars ? fmtN(a.stars) : ''], [t('License'), a.license], [t('By'), a.author],
  ].filter(([, v]) => v);
  const sys = (a.systems || []).filter((k) => OS_NAME[k]);
  return `<div class="stack">${S.tab === 'overview' ? readyCard(a) : ''}<div class="card"><dl class="dl">${facts.map(([k, v]) => `<div><dt>${esc(k)}</dt><dd>${k === t('By') ? `<span class="who">${whoHTML(a)}</span>` : esc(v)}</dd></div>`).join('')}
      ${sys.length ? `<div><dt>${esc(t('Runs on'))}</dt><dd style="display:flex;gap:10px;align-items:center;justify-content:flex-end">${sys.map((k) => `<span style="display:inline-flex;gap:6px;align-items:center">${osIcon(k)}${OS_NAME[k]}</span>`).join('')}</dd></div>` : ''}</dl></div>
    ${S.library[a.id] && S.os !== 'macos' ? `<div class="acts grid2">${S.steam ? `<button type="button" class="btn" data-act="steam" data-id="${esc(a.id)}">${esc(t('Add to Steam'))}</button>` : ''}<button type="button" class="btn${S.steam ? '' : ' span2'}" data-act="desktop" data-id="${esc(a.id)}">${esc(t('Add to desktop'))}</button></div>` : ''}
    ${a.repo ? `<div class="acts grid2"><button type="button" class="btn" data-act="url" data-url="https://github.com/${esc(a.repo)}">GitHub</button><button type="button" class="btn" data-act="url" data-url="https://github.com/${esc(a.repo)}/issues">${esc(t('Report a problem'))}</button></div>` : ''}</div>`;
}
function gameBody(a) {
  const id = a.id, inst = S.library[id];
  gameTabs(a);
  let body = '';
  if (S.tab === 'overview') {
    const feats = (a.features && a.features.length ? a.features : a.tags || []);
    const stats = inst
      ? [[t('Played'), playedFor(inst)], [t('Last played'), inst.last_played ? ago(inst.last_played).replace(/^./, (c) => c.toLocaleUpperCase(curLocale())) : t('Not yet')], [t('Version'), (inst.tag || t('Not known')) + (S.updates[id] ? ' · ' + t('update ready') : '')], [t('Installed'), date(inst.installed_at * 1000)]]
      : [[t('Released'), date(a.released) || '—'], [t('Downloads'), a.downloads ? fmtN(a.downloads) : '—'], [t('Stars'), a.stars ? fmtN(a.stars) : '—'], [t('Size'), a.size || '—']];
    body = `<div class="stack"><div class="stats">${stats.map(([k, v]) => `<div><span>${esc(k)}</span><b>${esc(v)}</b></div>`).join('')}</div>
      ${inst ? playCard(inst) : ''}
      ${window.betaCard ? window.betaCard(a) : ''}
      ${a.blurb ? `<p class="blurb">${esc(a.blurb)}</p>` : ''}
      ${versionsCard(a)}
      ${feats.length ? `<div class="feat">${feats.map((ft) => `<span>${esc(ft)}</span>`).join('')}</div>` : ''}
      ${aboutCard(a)}
      ${shotsCard(a)}
      ${runsCard(a)}
      ${a.description ? `<div class="card"><h3 style="margin-bottom:12px">${esc(t('From the developer'))}</h3><div class="md">${md(a.description)}</div></div>` : ''}
      ${bootNote(a) || setupGuide(a) ? `<div class="card"><h3 style="margin:0">${esc(t('How to Set Up'))}</h3>${setupGuide(a) ? `<p style="margin:8px 0 0">${esc(setupGuide(a))}</p>` : ''}${bootNote(a) ? `<p style="margin:8px 0 0">${esc(bootNote(a))}</p>` : ''}</div>` : ''}
      ${isAI(a) ? `<div class="card"><span class="ai-tag">${esc(t('AI-assisted'))}</span><p style="margin:10px 0 0">${esc(a.ai.by === 'developer' ? tf('Its developer says AI tools helped with its {uses}.', { uses: aiUses(a) }) : tf('Its project on GitHub shows AI tools helped with its {uses}.', { uses: aiUses(a) }))}</p>${a.ai.note ? `<p class="muted" style="margin:8px 0 0">“${esc(a.ai.note)}”</p>` : ''}<p class="muted" style="margin:8px 0 0;font-size:14px">${esc(S.settings.hide_ai ? t('You\'ve hidden AI-assisted recomps, so this one doesn\'t show anywhere else.') : t('Rather not see AI-assisted recomps?'))} <a href="#" data-go="settings">${esc(t('Settings'))}</a></p></div>` : ''}
      ${a.credits ? `<div class="card"><h3 style="margin-bottom:8px">${esc(t('Credits'))}</h3><div class="md">${md(a.credits)}</div></div>` : ''}
      ${shelf('like', t('More like this'), moreLike(a), { quick: true })}
    </div>`;
  } else if (S.tab === 'updates') {
    const r = S.rels[a.repo];
    if (!r) { loadRels(a.repo); body = `<div class="stack"><div class="card"><p class="muted" style="margin:0">${esc(t('Loading the releases…'))}</p></div></div>`; }
    else if (r.loading) body = `<div class="stack"><div class="card"><p class="muted" style="margin:0">${esc(t('Loading the releases…'))}</p></div></div>`;
    else if (r.error) body = `<div class="stack"><div class="card"><p style="margin:0">${esc(t(r.error))}</p></div></div>`;
    else if (!r.list.length) body = `<div class="stack"><div class="card"><p style="margin:0">${esc(t('No releases yet.'))}</p></div></div>`;
    else body = `<div class="stack">${inst && inst.pinned ? `<div class="card pinned"><div class="set-row"><div class="k"><b>${esc(tf('You\'re on {tag}, picked by you', { tag: inst.tag }))}</b><span>${esc(t('Updates wait until you go back to the newest version.'))}</span></div><button type="button" class="btn main" data-act="install" data-id="${esc(id)}">${esc(t('Go to the newest'))}</button></div></div>` : ''}
      <div class="card">${r.list.filter((x) => !x.draft).slice(0, 10).map((x, n) => `<div class="rel"><div class="rel-h"><h3>${esc(x.name || x.tag_name)}</h3><span class="when">${esc(date(x.published_at))}</span>${x.prerelease ? `<span class="pre">${esc(t('Pre-release'))}</span>` : ''}${inst && inst.tag === x.tag_name ? `<span class="ai-tag inst">${esc(t('Installed'))}</span>` : inst && !S.jobs[id] && !S.running.has(id) ? `<button type="button" class="btn sm" data-act="version" data-id="${esc(id)}" data-tag="${esc(x.tag_name)}">${esc(n === 0 ? t('Install this one') : t('Go back to this one'))}</button>` : ''}</div><div class="md">${x.body ? md(x.body) : `<p class="muted">${esc(t('No notes for this one.'))}</p>`}</div></div>`).join('')}</div></div>`;
  } else if (S.tab === 'saves') {
    body = savesTab(a);
  } else if (S.tab === 'dev') {
    body = devTab(a);
  } else if (S.tab === 'mods') {
    body = modsTab(a);
  } else if (S.tab === 'settings') {
    body = settingsTab(a);
  } else if (S.tab === 'looks') {
    body = looksTab(a);
  } else {
    const shots = S.shots[id];
    if (!shots) loadShots(id);
    body = `<div class="stack"><div class="card">
      <div class="set-row"><div class="k"><b>${esc(t('Folder'))}</b><span>${esc(inst.path)}</span></div><div class="acts"><button type="button" class="btn" data-act="folder" data-id="${esc(id)}">${esc(t('Open'))}</button>${inst.local ? '' : `<button type="button" class="btn" data-act="move" data-id="${esc(id)}">${esc(t('Move'))}</button>`}</div></div>
      <div class="set-row"><div class="k"><b>${esc(t('Shortcuts'))}</b><span>${esc(S.steam ? t('Start it from your desktop or from Steam, with its art. Play time still counts here.') : t('Start it from your desktop. Play time still counts here.'))}</span></div><div class="acts"><button type="button" class="btn" data-act="desktop" data-id="${esc(id)}">${esc(t('Add to desktop'))}</button>${S.steam ? `<button type="button" class="btn" data-act="steam" data-id="${esc(id)}">${esc(t('Add to Steam'))}</button>` : ''}</div></div>
      ${inst.local ? '' : `<div class="set-row"><div class="k"><b>${esc(t('Update automatically'))}</b><span>${esc(t('Installs new releases as soon as the launcher finds them. Your saves are backed up first.'))}</span></div><button type="button" class="switch" role="switch" aria-checked="${!!inst.auto_update}" aria-label="${esc(t('Update automatically'))}" data-act="auto-up" data-id="${esc(id)}"></button></div>`}
      <div class="set-row"><div class="k"><b>${esc(t('Launch options'))}</b><span>${inst.args ? esc(inst.args) : esc(t('Extra options passed to the game when it starts.'))}</span></div><button type="button" class="btn" data-act="launch-opts" data-id="${esc(id)}">${esc(inst.args ? t('Change') : t('Add'))}</button></div>
      <div class="set-row"><div class="k"><b>${esc(t('Program it starts'))}</b><span>${inst.exe ? esc(inst.exe) : esc(t('Not found. Pick it so Play knows what to start.'))}</span></div><button type="button" class="btn" data-act="setexe" data-id="${esc(id)}">${esc(t('Choose Program'))}</button></div>
      ${inst.local ? '' : `<div class="set-row"><div class="k"><b>${esc(t('Backups of your saves'))}</b><span>${esc(t('Taken before every update. Saves, settings and mods in its folder.'))}</span></div><button type="button" class="btn" data-act="backups" data-id="${esc(id)}">${esc(t('Backups'))}</button></div>`}
      ${inst.local ? '' : `<div class="set-row"><div class="k"><b>${esc(t('Installed version'))}</b><span>${inst.asset ? `${esc(inst.tag)} · ${esc(inst.asset)}${inst.size ? ' · ' + fmtB(inst.size) : ''} · ${esc(inst.verified ? t('matched GitHub\'s checksum') : t('GitHub had no checksum for it'))}` : esc(inst.tag || t('Not known. Updating puts the newest one in its folder.'))}</span></div></div>`}
      ${inst.local ? `<div class="set-row"><div class="k"><b>${esc(t('Remove from Library'))}</b><span>${esc(t('Your folder stays exactly as it is.'))}</span></div><button type="button" class="btn danger" data-act="dev-remove" data-id="${esc(id)}">${esc(t('Remove'))}</button></div>` : `<div class="set-row"><div class="k"><b>${esc(t('Uninstall'))}</b><span>${esc(t('Takes it out of your Library. You choose whether its folder goes too.'))}</span></div><button type="button" class="btn danger" data-act="uninstall" data-id="${esc(id)}">${esc(t('Uninstall'))}</button></div>`}
    </div>
    <div class="card"><div class="card-h" style="margin-bottom:14px"><h3>${esc(t('Screenshots'))}</h3><button type="button" class="btn sm" data-act="shots-folder" data-id="${esc(id)}">${esc(t('Open folder'))}</button></div>
      ${shots && shots.length ? `<div class="shots">${shots.slice(0, 8).map((f, i) => `<button type="button" class="shot" data-act="shot-g" data-id="${esc(id)}" data-i="${i}" data-path="${esc(f)}"><img src="${esc(fileSrc(f))}" alt="${esc(t('Screenshot'))}" loading="lazy"${LOADED.has(fileSrc(f)) ? ' class="ld"' : ''}></button>`).join('')}</div>` : `<p class="muted" style="margin:0">${esc(t('None yet. While you play, press F12, or View + RB on a controller.'))}</p>`}</div></div>`;
  }
  return body + gameRail(a);
}
// Switching tabs redraws only the part below them: the banner and cover stay where they are.
function showTab(k) {
  const id = S.arg, a = byId(id) || (S.library[id] && { id, name: S.library[id].name, repo: S.library[id].repo, systems: [S.os] });
  S.tab = k;
  const body = $('#g-body');
  if (!a || !body) return render(true);
  $$('.tabs-in button').forEach((b) => { const on = b.dataset.tab === k; b.classList.toggle('on', on); b.setAttribute('aria-selected', String(on)); });
  moveTabPill();
  body.classList.remove('still');
  body.innerHTML = gameBody(a);
  fitTitles(body);
  paintLooks();
}
function moveTabPill() {
  const pill = $('.tab-pill'), on = $('.tabs-in button.on');
  if (!pill || !on) return;
  pill.style.width = on.offsetWidth + 'px';
  pill.style.transform = `translateX(${on.offsetLeft - 4}px)`;
}

// ---------- setting a recomp up: the player's copy and the game's own settings ----------
async function loadSetup(id) {
  if (S.setup[id] === 'loading') return;
  S.setup[id] = 'loading';
  try { S.setup[id] = await invoke('game_setup', { id }); } catch (e) { S.setup[id] = null; }
  if (S.view === 'game' && S.arg === id) refreshGame();
  if (window.tvRefresh) window.tvRefresh();
}
// A game page's data changed: redraw its tabs and body, not its banner.
function refreshGame() {
  if (S.view !== 'game' || !$('#g-body') || ($('.gp') && $('.gp').dataset.gp !== S.arg)) return render(true);
  const id = S.arg, a = byId(id) || (S.library[id] && { id, name: S.library[id].name, repo: S.library[id].repo, systems: [S.os] });
  if (!a) { $('.gp').dataset.gp = ''; return render(true); }
  const tabs = gameTabs(a);
  const have = $$('.tabs-in button').map((b) => b.dataset.tab + (b.querySelector('.tab-beta') ? '+' : '')).join();
  if (have !== tabs.map(([k, , x]) => k + (x ? '+' : '')).join()) {
    $('.tabs-in').innerHTML = `<span class="tab-pill" aria-hidden="true"></span>${tabs.map(([k, n, x]) => `<button type="button" role="tab" class="${S.tab === k ? 'on' : ''}" aria-selected="${S.tab === k}" data-act="tab" data-tab="${k}">${esc(n)}${x || ''}</button>`).join('')}`;
  }
  $$('.tabs-in button').forEach((b) => { const on = b.dataset.tab === S.tab; b.classList.toggle('on', on); b.setAttribute('aria-selected', String(on)); });
  const acts = $(`.gp-acts`);
  if (acts) morph(acts, actions(a));
  const main = $('#main'), top = main.scrollTop;
  const body = $('#g-body');
  body.classList.add('still');
  morph(body, gameBody(a));
  main.scrollTop = top;
  moveTabPill();
  paintLooks();
  fitTitles(body);
  layoutShelves(body);
}
// Ask for the player's copy: a disc image, or a folder.
// What the file picker shows for a game's copy: every format its console's games come in, so
// nothing the player has is hidden behind a filter (N64 ROMs in any byte order, every disc format).
const ROM_EXTS = { n64: ['z64', 'n64', 'v64', 'zip', '7z'], disc: ['iso', 'xiso', 'img', 'bin', 'cue', 'chd', 'cso', 'ciso', 'rvz', 'wbfs', 'gcm', 'nkit', 'pbp', '7z', 'zip'] };
// Each console's files, for the picker (the catalog's console ids, a few spellings each).
const SYSTEM_EXTS = [
  [/^(nes|famicom)$/, ['nes', 'fds', 'unf', 'unif', 'zip']],
  [/^(snes|sfc|superfamicom)$/, ['sfc', 'smc', 'zip']],
  [/^(gb|gbc|gameboy|gameboycolor)$/, ['gbc', 'gb', 'zip']],
  [/^(gba|gameboyadvance)$/, ['gba', 'zip']],
  [/^(ds|nds)$/, ['nds', 'zip']],
  [/^(3ds|n3ds)$/, ['3ds', 'cci', 'cia', 'cxi', 'app', 'zip']],
  [/^(vb|virtualboy)$/, ['vb', 'vboy', 'zip']],
  [/^(switch|nx)$/, ['xci', 'nsp', 'nca']],
  [/^(genesis|md|megadrive|gen)$/, ['md', 'gen', 'smd', 'bin', 'zip']],
  [/^(32x|sega32x)$/, ['32x', 'bin', 'zip']],
  [/^(gg|gamegear)$/, ['gg', 'zip']],
  [/^(sms|mastersystem)$/, ['sms', 'zip']],
  [/^(arcade|mame|fbneo)$/, ['zip', '7z', 'chd']],
  [/^(cdi|cd-i|philipscdi)$/, ['chd', 'cue', 'bin', 'iso']],
  [/^(amiga|amigacd32|cd32)$/, ['adf', 'hdf', 'ipf', 'lha', 'dms', 'zip']],
];
const extsFor = (origin) => { const o = String(origin || '').toLowerCase().replace(/[\s_.]/g, ''); const m = SYSTEM_EXTS.find(([re]) => re.test(o)); return m ? m[1] : null; };
function copyPickOpts(id) {
  const a = byId(id) || { id };
  // A console with files of its own (cartridge ROMs, Switch games, Amiga disks): those.
  const own = extsFor(a.origin);
  if (own) return { title: t('Your copy of the game'), directory: false, multiple: false, filters: [{ name: tf('{console} files', { console: originName(a.origin) }), extensions: own }, { name: t('All files'), extensions: ['*'] }] };
  const b = (a && a.boot) || (S.bootBuiltin || {})[id] || {};
  const su = S.setup[id] && S.setup[id] !== 'loading' ? S.setup[id] : {};
  const what = `${(b.files || {}).what || ''} ${su.needs || ''}`;
  const n64 = /\bN64\b|\.z64|\.n64|\.v64|\bROM\b/i.test(what) && !/\bdisc\b|\.iso/i.test(what);
  const exts = n64 ? ROM_EXTS.n64 : ROM_EXTS.disc;
  return { title: n64 ? t('Your ROM of the game') : t('Your disc image of the game'), directory: false, multiple: false,
    filters: [{ name: n64 ? t('N64 ROMs') : t('Disc images'), extensions: exts }, { name: t('All files'), extensions: ['*'] }] };
}
async function pickCopy(id, folder) {
  const su = S.setup[id] || {};
  const p = await pickFile(folder ? { title: t('The game\'s folder'), directory: true } : copyPickOpts(id)).catch(fail);
  if (!p) return false;
  const path = typeof p === 'string' ? p : p.path;
  if (/\.(7z|zip)$/i.test(path)) toast(t('Unpacking your copy…'));
  try { await invoke('set_game_copy', { id, path }); } catch (e) { fail(e); return false; }
  delete S.setup[id];
  await loadSetup(id);
  toast(t('Got it. Press Play.'));
  return true;
}
// The game's update, when it needs one beside the copy and it isn't in yet.
function needUpdate(su) { return !!(su && su.update && !su.update_copy && !su.update_optional); }
async function pickUpdate(id, folder) {
  const p = await pickFile(folder ? { title: t('The update\'s folder'), directory: true } : { title: t('The game\'s update'), directory: false, multiple: false, filters: [{ name: t('All files'), extensions: ['*'] }] }).catch(fail);
  if (!p) return null;
  try { await invoke('set_update_copy', { id, path: typeof p === 'string' ? p : p.path }); } catch (e) { return String(e && e.message ? e.message : e); }
  delete S.setup[id];
  try { S.setup[id] = await invoke('game_setup', { id }); } catch (e) { S.setup[id] = null; }
  return '';
}
// ---------- setting a game up with the player's copy ----------
const SU = { id: null, el: null, stage: 'pick', step: '', ev: {}, steps: {}, q: [], at: 0, path: '', extras: false, result: null, err: '' };
async function setupModal(id) {
  if (document.body.classList.contains('tv-on') && window.tvSetup) return window.tvSetup(id);
  if (SU.el && SU.stage === 'run' && SU.id !== id) return toast(t('Finish the setup that\'s running first.'));
  if (S.setup[id] === undefined || S.setup[id] === 'loading') { try { S.setup[id] = await invoke('game_setup', { id }); } catch (e) { S.setup[id] = null; } }
  const su = S.setup[id];
  if (!su || !su.needs) return toast(t('This game asks for your copy itself the first time it starts.'));
  closeSetup(true);
  Object.assign(SU, { id, stage: needUpdate(su) && su.copy ? 'update' : 'pick', step: '', ev: {}, steps: {}, q: [], at: 0, path: '', extras: false, result: null, err: '', uerr: '' });
  const s = document.createElement('div');
  s.className = 'scrim';
  s.innerHTML = `<div class="modal su" role="dialog" aria-modal="true" aria-label="${esc(t('Set up a game'))}"></div>`;
  document.body.append(s);
  SU.el = s;
  s.addEventListener('click', (e) => {
    if (e.target === s && SU.stage !== 'run') return closeSetup();
    const b = e.target.closest('[data-su]');
    if (b) suAct(b);
  });
  suDraw();
  const f = $('.pick, .found .btn', s);
  if (f) f.focus();
}
function closeSetup(quiet) {
  if (!SU.el) return;
  if (SU.stage === 'run' && SU.id) invoke('setup_cancel', { id: SU.id }).catch(() => {});
  SU.el.remove();
  SU.el = null;
  if (!quiet && SU.stage !== 'pick') { delete S.setup[SU.id]; loadSetup(SU.id); }
}
async function suAct(b) {
  const id = SU.id, su = S.setup[id] || {};
  switch (b.dataset.su) {
    case 'close': return closeSetup();
    case 'found': return suBegin(S.owned[id].path);
    case 'iso': case 'folder': {
      const folder = b.dataset.su === 'folder';
      const p = await pickFile(folder ? { title: t('The game\'s folder'), directory: true } : copyPickOpts(id)).catch(fail);
      if (p) suBegin(typeof p === 'string' ? p : p.path);
      return;
    }
    case 'again': SU.stage = 'pick'; SU.err = ''; return suDraw();
    case 'ufile': case 'ufolder': {
      const why = await pickUpdate(id, b.dataset.su === 'ufolder');
      if (why === null) return;
      if (why) { SU.uerr = why; return suDraw(); }
      SU.uerr = ''; SU.stage = 'ready'; SU.result = SU.result || { text: t('Checked') };
      return suDraw();
    }
    case 'uskip': SU.stage = 'ready'; return suDraw();
    case 'play': closeSetup(); return playGame(id);
    case 'settings': closeSetup(); if (S.view === 'game' && S.arg === id) return showTab('settings'); S.nextTab = 'settings'; return go('game', id);
  }
  void su;
}
async function suBegin(path) {
  Object.assign(SU, { stage: 'run', path, step: 'read', steps: {}, ev: {}, q: [], at: Date.now(), result: null, err: '' });
  suDraw();
  try { await invoke('setup_copy', { id: SU.id, path }); } catch (e) { SU.stage = 'error'; SU.err = String(e && e.message ? e.message : e); suDraw(); }
}
const SU_ORDER = ['read', 'verify', 'extras', 'setup', 'done'];
// The copy's in: the game's update next, when it needs one, then ready.
async function suAfterCopy() {
  const id = SU.id;
  try { S.setup[id] = await invoke('game_setup', { id }); } catch (e) { /* keeps the one it has */ }
  if (!SU.el || SU.id !== id) return;
  const su = S.setup[id];
  SU.stage = su && su.update && !su.update_copy ? 'update' : 'ready';
  suDraw();
}
// Steps that finish quickly still show for a moment, so each one can be seen.
function suPump() {
  clearTimeout(SU.timer);
  while (SU.q.length) {
    const ev = SU.q[0];
    if (ev.step === 'error') { SU.q = []; SU.stage = 'error'; SU.err = ev.text; return suDraw(); }
    const cur = SU_ORDER.indexOf(SU.step), next = SU_ORDER.indexOf(ev.step);
    if (next > cur) {
      const wait = 700 - (Date.now() - SU.at);
      if (wait > 0) { SU.timer = setTimeout(suPump, wait); return; }
      SU.steps[SU.step] = Object.assign({}, SU.steps[SU.step], { done: true });
      SU.step = ev.step; SU.at = Date.now();
    }
    SU.q.shift();
    SU.extras = ev.extras;
    SU.steps[ev.step] = Object.assign({}, SU.steps[ev.step], ev);
    if (ev.step === 'done') { SU.result = ev; return suAfterCopy(); }
  }
  suDraw();
}
T.event.listen('setup', ({ payload: p }) => { if (SU.el && p.id === SU.id && SU.stage === 'run') { SU.q.push(p); suPump(); } });
function suDraw() {
  if (!SU.el) return;
  const id = SU.id, a = byId(id) || { name: (S.library[id] || {}).name || id }, su = S.setup[id] || {};
  const box = $('.modal', SU.el);
  const game = a.game || a.name;
  const cov = a.cover ? media(a.cover) : '';
  const discCls = SU.stage === 'pick' ? 'sdisc ghost' : SU.stage === 'run' ? (SU.step === 'read' && Date.now() - SU.at < 900 ? 'sdisc in' : 'sdisc spin') : SU.stage === 'ready' || SU.stage === 'update' ? 'sdisc done' : 'sdisc';
  const label = `<div class="lab">${cov ? pic(a.cover) : ''}</div><div class="sheen"></div><div class="hub"></div><div class="hole"></div>`;
  let right = '';
  if (SU.stage === 'pick') {
    const own = S.owned[id];
    const kind = su.kind || 'iso';
    const picks = [kind !== 'folder' ? `<button type="button" class="pick" data-su="iso">${DISC_L}<b>${esc(t('Disc image'))}</b><span>${esc(t('An .iso of your disc'))}</span></button>` : '', kind === 'iso_or_folder' || kind === 'folder' ? `<button type="button" class="pick" data-su="folder"><svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" aria-hidden="true"><path d="M3 6.5A1.5 1.5 0 0 1 4.5 5H10l2 2.5h7.5A1.5 1.5 0 0 1 21 9v9.5a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 18.5z"/></svg><b>${esc(t('Game folder'))}</b><span>${esc(t('A folder you copied from the disc'))}</span></button>` : ''].filter(Boolean);
    right = `<span class="kick">${esc(t('Set up'))} · ${esc(a.name)}</span><h2>${esc(tf('Insert your copy of {game}', { game }))}</h2><p>${esc(su.needs || '')} ${esc(t('It\'s checked and set up once. After that, you just press Play.'))}</p>
      ${own ? `<div class="found">${DISC_L}<div class="k"><b>${esc(t('Found in your folders'))}</b><span>${esc(own.path)}</span></div><button type="button" class="btn sm main" data-su="found">${esc(t('Use this'))}</button></div>` : ''}
      <div class="picks${picks.length === 1 ? ' one' : ''}">${picks.join('')}</div>`;
  } else if (SU.stage === 'run') {
    const order = ['read', 'verify'].concat(SU.extras ? ['extras'] : [], ['setup']);
    const st = SU.steps;
    const sub = {
      read: st.read && st.read.text ? st.read.text : baseName(SU.path),
      verify: st.verify && st.verify.text ? st.verify.text : st.verify && st.verify.total ? tf('{done} of {total}', { done: fmtB(st.verify.done), total: fmtB(st.verify.total) }) : t('Reading all of it, to make sure it\'s complete'),
      extras: (st.extras && st.extras.text) || t('What the game needs from your disc'),
      setup: t('Its settings, your controller and where your copy is'),
    };
    const names = { read: t('Reading your disc'), verify: t('Checking your copy'), extras: t('Getting the extras'), setup: t('Setting it up') };
    const weight = { read: 5, verify: SU.extras ? 70 : 80, extras: 15, setup: 10 };
    let total = 0, have = 0;
    for (const k of order) { total += weight[k]; const s = st[k]; const i = order.indexOf(k), cur = order.indexOf(SU.step); have += i < cur || (s && s.done) ? weight[k] : i === cur ? weight[k] * Math.min(1, (s && s.pct) || 0) : 0; }
    const barW = Math.round((have / total) * 100);
    // Only the numbers move while a step runs; the list itself is redrawn when a step changes.
    if (box.dataset.stage === 'run' && box.dataset.step === SU.step + order.length && $('.steps', box)) {
      $$('.step', box).forEach((el, i) => { const s = $('.k span', el); if (s) s.textContent = sub[order[i]]; });
      $('.su-bar i', box).style.width = barW + '%';
      return;
    }
    box.dataset.step = SU.step + order.length;
    right = `<span class="kick">${esc(t('Setting up'))}</span><h2>${esc(a.name)}</h2><p style="white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${esc(SU.path)}</p>
      <div class="steps">${order.map((k) => { const i = order.indexOf(k), cur = order.indexOf(SU.step); const done = i < cur, now = i === cur; return `<div class="step${!done && !now ? ' wait' : ''}">${done ? `<span class="ok">${CHECK}</span>` : now ? '<span class="spinr"></span>' : '<span class="todo"></span>'}<div class="k"><b>${esc(names[k])}</b><span>${esc(sub[k])}</span></div></div>`; }).join('')}</div>
      <div class="su-bar"><i style="width:${barW}%"></i></div>`;
  } else if (SU.stage === 'update') {
    const picks = [su.update_file ? `<button type="button" class="pick" data-su="ufile"><svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" aria-hidden="true"><path d="M14 3H7a1.5 1.5 0 0 0-1.5 1.5v15A1.5 1.5 0 0 0 7 21h10a1.5 1.5 0 0 0 1.5-1.5V7.5z"/><path d="M14 3v4.5h4.5"/></svg><b>${esc(t('Update File'))}</b><span>${esc(t('The file, as it came'))}</span></button>` : '', su.update_folder ? `<button type="button" class="pick" data-su="ufolder"><svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" aria-hidden="true"><path d="M3 6.5A1.5 1.5 0 0 1 4.5 5H10l2 2.5h7.5A1.5 1.5 0 0 1 21 9v9.5a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 18.5z"/></svg><b>${esc(t('Update Folder'))}</b><span>${esc(t('A folder with its files'))}</span></button>` : ''].filter(Boolean);
    right = `<span class="kick">${esc(t('Set up'))} · ${esc(a.name)}</span><h2>${esc(t('Add the Game\'s Update'))}</h2><p>${esc(su.update || '')} ${esc(t('It goes in with your copy, once.'))}</p>
      ${SU.uerr ? `<p class="su-err">${esc(t(SU.uerr))}</p>` : ''}
      <div class="picks${picks.length === 1 ? ' one' : ''}">${picks.join('')}</div>
      ${su.update_optional ? `<button type="button" class="btn" data-su="uskip" style="margin-top:12px">${esc(t('Skip for Now'))}</button>` : ''}`;
  } else if (SU.stage === 'ready') {
    const r = SU.result || {};
    right = `<span class="su-ok${r.verified ? '' : ' plain'}">${CHECK}${esc(r.text || t('Checked'))}</span><h2 style="font-size:32px">${esc(t('Ready to play'))}</h2>
      <p>${esc(tf('{name} is set up with your copy. Change how it looks and runs any time in its settings.', { name: a.name }))}</p>
      <div class="picks" style="margin-top:auto"><button type="button" class="btn main big" data-su="play">${PLAY}${esc(t('Play'))}</button><button type="button" class="btn big" data-su="settings">${esc(t('Settings'))}</button></div>`;
  } else {
    right = `<span class="kick">${esc(t('Set up'))} · ${esc(a.name)}</span><h2>${esc(t('That copy didn\'t work'))}</h2><p class="su-err">${esc(t(SU.err))}</p>
      <div class="picks" style="margin-top:auto"><button type="button" class="btn main big" data-su="again">${esc(t('Try another copy'))}</button><button type="button" class="btn big" data-su="close">${esc(t('Close'))}</button></div>`;
  }
  const keepDisc = $('.sdisc', box);
  const stageHtml = SU.stage === 'pick' ? `<div class="su-drop"><div class="sdisc ghost"></div><b>${esc(t('Drop your disc image here'))}</b><span>${esc(t('or choose it on the right'))}</span></div>` : `<div class="${discCls}">${label}</div>`;
  if (!box.firstChild || box.dataset.stage !== SU.stage) {
    if (SU.stage !== 'run') box.dataset.step = '';
    box.dataset.stage = SU.stage;
    box.innerHTML = `${cov ? `<div class="su-glow" style="background-image:url('${cssUrl(cov)}')"></div>` : ''}<button type="button" class="su-x" data-su="close" aria-label="${esc(t('Close'))}">${XMARK}</button>
      <div class="su-in"><div class="su-stage">${stageHtml}<div class="drive"><i class="${SU.stage === 'run' ? 'on' : ''}"></i></div></div><div class="su-r">${right}</div></div>`;
    const f = $('.su-r .btn.main, .su-r .pick, .su-r .btn', box) || $('.su-x', box);
    if (f) f.focus({ preventScroll: true });
  } else {
    $('.su-r', box).innerHTML = right;
    const d = $('.sdisc', box);
    if (d && keepDisc && SU.stage === 'run' && SU.step !== 'read' && !d.classList.contains('spin')) d.className = 'sdisc spin';
  }
}
T.event.listen('tauri://drag-drop', ({ payload: p }) => {
  const paths = (p && p.paths) || [];
  if (SU.el) { $('.su-stage', SU.el)?.classList.remove('hot'); if (SU.stage === 'pick' && paths[0]) suBegin(paths[0]); }
});
T.event.listen('tauri://drag-enter', () => { if (SU.el && SU.stage === 'pick') $('.su-stage', SU.el)?.classList.add('hot'); });
T.event.listen('tauri://drag-leave', () => { if (SU.el) $('.su-stage', SU.el)?.classList.remove('hot'); });
function settingsTab(a) {
  const id = a.id, su = S.setup[id];
  if (!su || su === 'loading') return `<div class="stack"><div class="card"><p class="muted" style="margin:0">${esc(t('Loading…'))}</p></div></div>`;
  const seg = (o) => o.choices.length <= 4 && o.choices.join('').length <= 34
    ? `<div class="seg" role="radiogroup" aria-label="${esc(o.label)}">${o.choices.map((c, i) => `<button type="button" role="radio" aria-checked="${o.current === i}" data-act="gset" data-id="${esc(id)}" data-k="${esc(o.id)}" data-i="${i}"${o.applies ? '' : ' disabled'}>${esc(c)}</button>`).join('')}</div>`
    : `<select class="sel" data-act="gsel" data-id="${esc(id)}" data-k="${esc(o.id)}"${o.applies ? '' : ' disabled'}>${o.choices.map((c, i) => `<option value="${i}"${o.current === i ? ' selected' : ''}>${esc(c)}</option>`).join('')}</select>`;
  return `<div class="stack">
    ${su.needs ? `<div class="card"><div class="set-row"><div class="k"><b>${esc(t('Your copy of the game'))}</b><span>${su.copy ? esc(su.copy) : esc(su.needs)}</span></div><button type="button" class="btn${su.copy ? '' : ' main'}" data-act="setup" data-id="${esc(id)}">${esc(su.copy ? t('Change') : t('Insert it'))}</button></div></div>` : ''}
    ${su.update ? `<div class="card"><div class="set-row"><div class="k"><b>${esc(t('The game\'s update'))}</b><span>${su.update_copy ? esc(su.update_copy) : esc(su.update)}</span></div><button type="button" class="btn${su.update_copy || su.update_optional ? '' : ' main'}" data-act="setupdate" data-id="${esc(id)}">${esc(su.update_copy ? t('Change') : t('Choose It'))}</button></div></div>` : ''}
    ${su.settings.length ? `<div class="card"><div class="card-h"><h3>${esc(t('How it runs'))}</h3><span>${esc(su.waiting ? t('Play it once first, then change them here.') : t('Saved in the game\'s own settings'))}</span></div>
      ${su.settings.map((o) => `<div class="set-row${o.applies ? '' : ' off'}"><div class="k"><b>${esc(o.label)}</b><span>${esc(o.help)}</span></div>${seg(o)}</div>`).join('')}</div>` : ''}
    ${su.actions.length ? `<div class="card">${su.actions.map((x) => `<div class="set-row"><div class="k"><b>${esc(x.label)}</b><span>${esc(x.help)}</span></div><button type="button" class="btn${x.kind === 'clear' ? ' danger' : ''}" data-act="gaction" data-id="${esc(id)}" data-k="${esc(x.id)}">${esc(x.kind === 'clear' ? t('Clear') : t('Open'))}</button></div>`).join('')}</div>` : ''}
  </div>`;
}
// Settings > General > Portable: whether this launcher keeps its data beside itself, or a button
// to make a copy that does (on a portable drive, to use on other PCs).
function portableRow(row) {
  if (S.portableInfo === undefined) {
    S.portableInfo = null;
    invoke('portable_info').then((v) => { S.portableInfo = v; if (S.view === 'settings') render(true); }).catch(() => {});
  }
  const p = S.portableInfo;
  // Marked Beta, like the other features still settling in.
  const brow = (b, s2, ctl) => `<div class="set-row"><div class="k"><b>${esc(b)}${BETA_MARK()}</b><span>${esc(s2)}</span></div>${ctl}</div>`;
  if (p && p.portable) return brow(t('Portable Mode'), tf('On. Your library, settings, play time and save backups are in {folder}, next to the launcher, so they go wherever it goes.', { folder: p.data }), `<button type="button" class="btn" data-act="portable-open">${esc(t('Open'))}</button>`);
  return brow(t('Make a Portable Copy'), t('A copy of RecompLauncher on a portable drive that keeps its library, settings, play time and save backups next to it, to use on any PC. Put your Recomp Library on that drive too.'), `<button type="button" class="btn" data-act="portable-make">${esc(t('Make a Portable Copy'))}</button>`);
}
async function setGameSetting(id, k, i) {
  try { await invoke('set_game_setting', { id, setting: k, choice: Number(i) }); } catch (e) { return fail(e); }
  delete S.setup[id];
  S.setup[id] = 'loading';
  try { S.setup[id] = await invoke('game_setup', { id }); } catch (e) { S.setup[id] = null; }
  if (S.view === 'game' && S.arg === id && $('#g-body')) showTab(S.tab); else render(true);
  if (window.tvRefresh) window.tvRefresh();
}

// ---------- looks: ReShade filters, previewed on the game's own picture ----------
async function loadLooks(id) {
  if (S.looks[id] === 'loading') return;
  S.looks[id] = 'loading';
  try { S.looks[id] = await invoke('looks_info', { id }); } catch (e) { S.looks[id] = { ok: false, why: String(e && e.message ? e.message : e) }; }
  await Looks.ready;
  if (S.view === 'game' && S.arg === id && S.tab === 'looks') refreshGame();
  if (window.tvRefresh) window.tvRefresh();
}
function forgetLooks(id) {
  if (S.looks[id] !== 'loading') delete S.looks[id];
  delete LOOK_PIC[id];
}
const LOOK_NONE = { id: '', name: N_('Original'), blurb: N_('The game as it comes, with nothing on top.') };
function looksTab(a) {
  const id = a.id, li = S.looks[id];
  if (li === undefined) loadLooks(id);
  if (Looks.failed()) return `<div class="stack"><div class="card"><h3>${esc(t('Looks didn\'t load'))}</h3><p class="muted" style="margin:8px 0 0">${esc(t('Restart the launcher to try again.'))}</p></div></div>`;
  if (!li || li === 'loading' || !Looks.defs()) return `<div class="stack"><div class="card"><p class="muted" style="margin:0">${esc(t('Loading…'))}</p></div></div>`;
  if (!li.ok) return `<div class="stack"><div class="card"><h3>${esc(t('Looks aren\'t available for this game'))}</h3><p class="muted" style="margin:8px 0 0">${esc(t(li.why))}</p></div></div>`;
  const D = Looks.defs(), cur = li.look || '', look = D.looks.find((l) => l.id === cur);
  const all = [LOOK_NONE].concat(D.looks);
  const crt = look && look.lines;
  const origin = originName(a.origin);
  const src = (li.shot ? t('Your latest screenshot.') : t('The game\'s banner. Take a screenshot while you play (F12) to see looks on the game itself.')) + (look ? ' ' + t('Drag across the picture to compare.') : '');
  return `<div class="stack lk" data-lk="${esc(id)}">
    <div class="lk-view${look ? '' : ' none'}" style="--x:50%">
      <canvas class="lk-after" aria-hidden="true"></canvas>
      <canvas class="lk-before" aria-hidden="true"></canvas>
      <div class="lk-split" aria-hidden="true"><i></i></div>
      <span class="lk-tag b">${esc(t('Original'))}</span><span class="lk-tag a">${esc(look ? t(look.name) : t('Original'))}</span>
      <input class="lk-range" type="range" min="0" max="100" value="50" aria-label="${esc(t('Compare with the original'))}" data-lk-split>
      <p class="lk-nopic">${esc(t('No picture to show it on yet. Take a screenshot while you play (F12).'))}</p>
    </div>
    <p class="lk-src">${esc(src)}</p>
    <div class="lk-grid" role="radiogroup" aria-label="${esc(t('Look'))}">${all.map((l) => `<button type="button" role="radio" class="lk-card" aria-checked="${l.id === cur}" data-act="look" data-id="${esc(id)}" data-look="${esc(l.id)}"><span class="lk-th"><canvas data-look-th="${esc(l.id)}"></canvas></span><b>${esc(t(l.name))}</b><span>${esc(t(l.blurb))}</span></button>`).join('')}</div>
    <div class="card">
      <div class="set-row${look ? '' : ' off'}"><div class="k"><b>${esc(t('Amount'))}</b><span>${esc(look ? tf('How strong {look} is.', { look: look.name === 'CRT' ? t('the CRT look') : t(look.name) === look.name ? look.name.toLowerCase() : t(look.name) }) : t('Pick a look first.'))}</span></div>
        <div class="lk-amt"><span>${esc(t('Light'))}</span><input type="range" min="0" max="100" step="5" value="${li.amount}" data-lk-amount="${esc(id)}"${look ? '' : ' disabled'} aria-label="${esc(t('Amount'))}"><span>${esc(t('Strong'))}</span></div></div>
      ${crt ? `<div class="set-row"><div class="k"><b>${esc(t('Scanlines'))}</b><span>${esc(origin ? tf('{n} lines, like the {console} sent to a TV.', { n: li.lines, console: origin }) : tf('{n} lines, like the console sent to a TV.', { n: li.lines }))}</span></div></div>` : ''}
      ${S.os === 'linux' ? `<div class="set-row"><div class="k"><b>${esc(t('In the game'))}</b><span>${esc(t('Press Home while you play to turn the look off and on again.'))}</span></div></div>
      <div class="set-row"><div class="k"><b>vkBasalt</b><span>${esc(t('On Linux, looks run on vkBasalt, from your distro\'s packages. The launcher turns it on only for the game you start.'))}</span></div><button type="button" class="btn sm" data-url="https://github.com/DadSchoorse/vkBasalt">GitHub</button></div>` : `<div class="set-row"><div class="k"><b>${esc(t('In the game'))}</b><span>${esc(t('Press Home while you play to fine-tune it in ReShade\'s menu. Changes made there stay until you pick another look here.'))}</span></div></div>
      <div class="set-row"><div class="k"><b>ReShade${li.reshade ? ' ' + esc(li.reshade) : ''}</b><span>${esc(t('Looks run on ReShade by crosire.'))} ${esc(li.reshade ? t('The launcher keeps it up to date.') : t('It downloads the first time you play with a look, about 5 MB.'))} ${esc(t('Nothing is installed into Windows.'))}</span></div><button type="button" class="btn sm" data-url="https://reshade.me">reshade.me</button></div>`}
    </div></div>`;
}
// The picture looks are shown on: the player's screenshot, or the game's banner.
const LOOK_PIC = {};
function lookPicture(id) {
  if (LOOK_PIC[id]) return LOOK_PIC[id];
  const li = S.looks[id] || {}, a = byId(id) || {};
  LOOK_PIC[id] = (async () => {
    let src = '';
    if (li.shot) { try { src = await invoke('look_picture', { path: li.shot }); } catch (e) { src = ''; } }
    if (!src) src = media(a.hero || a.cover || '');
    if (!src) return null;
    const img = new Image();
    img.crossOrigin = 'anonymous';
    img.src = src;
    try { await img.decode(); } catch (e) { return null; }
    return img;
  })();
  // A picture that didn't load is tried again next time.
  LOOK_PIC[id].then((img) => { if (!img) delete LOOK_PIC[id]; });
  return LOOK_PIC[id];
}
function sizeCanvas(c, ratio) {
  const w = Math.max(1, Math.round(c.clientWidth * devicePixelRatio));
  c.width = w;
  c.height = Math.max(1, Math.round(w / ratio));
}
let lookFrame = 0;
async function paintLooks(only) {
  const box = $('.lk[data-lk]');
  if (!box || !window.Looks) return;
  const id = box.dataset.lk, li = S.looks[id];
  if (!li || li === 'loading') return;
  const img = await lookPicture(id);
  if (!box.isConnected) return;
  const view = $('.lk-view', box), after = $('.lk-after', box), before = $('.lk-before', box);
  view.classList.toggle('nopic', !img);
  if (!img) return;
  const amount = Number(($('[data-lk-amount]', box) || {}).value || li.amount);
  if (!only || only === 'view') {
    sizeCanvas(after, 16 / 9); sizeCanvas(before, 16 / 9);
    if (!Looks.draw(after, img, li.look || '', amount, li.lines)) { view.classList.add('nogl'); }
    Looks.draw(before, img, '', 0, li.lines);
  }
  if (!only) {
    // Thumbnails show the middle of the picture, closer, so the looks can be told apart.
    for (const c of $$('[data-look-th]', box)) { sizeCanvas(c, 16 / 9); Looks.draw(c, img, c.dataset.lookTh, c.dataset.lookTh === (li.look || '') ? amount : 70, li.lines, 1.7); }
  }
}
async function setLook(id, look, amount) {
  const li = S.looks[id];
  if (!li || li === 'loading') return;
  const was = { look: li.look, amount: li.amount };
  li.look = look;
  if (amount != null) li.amount = amount;
  try { await invoke('set_look', { id, look, amount: amount != null ? amount : null }); } catch (e) { Object.assign(li, was); fail(e); }
  if (S.library[id]) { S.library[id].look = li.look; S.library[id].look_amount = li.amount; }
  if (amount == null) refreshGame();
  if (window.tvRefresh) window.tvRefresh();
}
document.addEventListener('input', (e) => {
  const el = e.target;
  if (el.matches('[data-lk-split]')) { el.closest('.lk-view').style.setProperty('--x', el.value + '%'); return; }
  if (el.matches('[data-lk-amount]')) {
    cancelAnimationFrame(lookFrame);
    lookFrame = requestAnimationFrame(() => paintLooks('view'));
  }
});
document.addEventListener('change', (e) => {
  const el = e.target;
  if (el.matches('[data-lk-amount]')) {
    const id = el.dataset.lkAmount, v = Number(el.value);
    setLook(id, (S.looks[id] || {}).look || '', v).then(() => paintLooks());
  }
});
addEventListener('resize', () => { if (S.view === 'game' && S.tab === 'looks') { cancelAnimationFrame(lookFrame); lookFrame = requestAnimationFrame(() => paintLooks()); } });

// ---------- mods ----------
async function loadMods(id, force) {
  if (S.mods[id] === 'loading') return;
  if (!force && S.mods[id] !== undefined) return;
  S.mods[id] = S.mods[id] && force ? S.mods[id] : 'loading';
  try { S.mods[id] = await invoke('mods_state', { id }); } catch (e) { S.mods[id] = null; }
  if (S.view === 'game' && S.arg === id) refreshGame();
  if (window.tvRefresh) window.tvRefresh();
}
async function loadModList(id) {
  if (S.modList[id]) return;
  S.modList[id] = { loading: true, list: [] };
  try { S.modList[id] = { list: (await invoke('mods_online', { id })) || [] }; } catch (e) { S.modList[id] = { list: [], error: String(e) }; }
  if (S.view === 'game' && S.arg === id) refreshGame();
  if (window.tvRefresh) window.tvRefresh();
}
const modBusy = {};
async function installMod(id, full) {
  modBusy[full] = 'in';
  render(true); if (window.tvRefresh) window.tvRefresh();
  try { const got = await invoke('mod_install', { id, full }); toast(got.length > 1 ? tf('Installed {name} and {n} it needs.', { name: got[0].name, n: got.length - 1 }) : got[0] ? tf('Installed {name}. Turn it on in the game\'s Mods menu if it isn\'t.', { name: got[0].name }) : t('Installed it. Turn it on in the game\'s Mods menu if it isn\'t.')); }
  catch (e) { fail(e); }
  delete modBusy[full];
  await loadMods(id, true);
}
async function removeMod(id, full) {
  modBusy[full] = 'out';
  render(true); if (window.tvRefresh) window.tvRefresh();
  try { await invoke('mod_remove', { id, full }); } catch (e) { fail(e); }
  delete modBusy[full];
  await loadMods(id, true);
}
function modsTab(a) {
  const id = a.id, m = S.mods[id], inst = S.library[id];
  if (!m || m === 'loading') return `<div class="loading" style="padding:40px">${esc(t('Loading…'))}</div>`;
  const both = !!(m.community && m.gamebanana);
  const src = m.community && (!m.gamebanana || (S.modSrc[id] || 'ts') === 'ts') ? 'ts' : 'gb';
  const have = new Set((m.installed || []).map((x) => x.full));
  const busy = (full) => modBusy[full] === 'in' ? (modProg[full] ? tf('Installing… {pct}%', { pct: modProg[full] }) : t('Installing…')) : t('Removing…');
  const btn = (p) => modBusy[p.full] ? `<button type="button" class="btn sm" disabled>${esc(busy(p.full))}</button>`
    : have.has(p.full) ? `<button type="button" class="btn sm" data-act="mod-remove" data-id="${esc(id)}" data-full="${esc(p.full)}">${esc(t('Remove'))}</button>`
      : `<button type="button" class="btn sm main" data-act="mod-install" data-id="${esc(id)}" data-full="${esc(p.full)}"${inst ? '' : ` disabled title="${esc(t('Install the game first'))}"`}>${esc(t('Install'))}</button>`;
  const from = (x) => (String(x.full).startsWith('gb:') ? 'GameBanana' : 'Thunderstore');
  const mine = (m.installed || []);
  const yours = `<div class="card"><div class="sec-h" style="margin-bottom:8px"><h3>${esc(t('Your mods'))}</h3>${inst ? `<button type="button" class="btn" data-act="mods-folder" data-id="${esc(id)}">${esc(t('Open the mods folder'))}</button>` : ''}</div>
      ${!inst ? `<p class="muted" style="margin:0">${esc(t('Install the game first, then add mods here.'))}</p>`
        : mine.length || (m.others || []).length
          ? mine.filter((x) => !x.dep).map((x) => `<div class="set-row"><div class="k"><b>${esc(x.name)}</b><span>${esc(tf('Version {v} · from {source}', { v: x.version, source: from(x) }))}</span></div>${btn({ full: x.full })}</div>`).join('')
            + mine.filter((x) => x.dep).map((x) => `<div class="set-row"><div class="k"><b>${esc(x.name)}</b><span>${esc(tf('Version {v} · another mod needs it', { v: x.version }))}</span></div>${btn({ full: x.full })}</div>`).join('')
            + (m.others || []).map((f) => `<div class="set-row"><div class="k"><b>${esc(f)}</b><span>${esc(t('Added by hand'))}</span></div></div>`).join('')
          : `<p class="muted" style="margin:0">${esc(t('No mods yet. Pick some below.'))}</p>`}
      ${inst ? `<p class="muted" style="margin:12px 0 0;font-size:13px">${esc(m.gamebanana && !m.community ? t('Mods from GameBanana are turned on when they install. Take one out here to turn it off.') : t('Turn mods on and off, and change their options, in the game\'s own Mods menu.'))}</p>` : ''}</div>`;
  const pick = both ? `<div class="seg" role="radiogroup" aria-label="${esc(t('Where mods come from'))}"><button type="button" role="radio" aria-checked="${src === 'ts'}" data-act="mod-src" data-id="${esc(id)}" data-v="ts">Thunderstore</button><button type="button" role="radio" aria-checked="${src === 'gb'}" data-act="mod-src" data-id="${esc(id)}" data-v="gb">GameBanana</button></div>` : '';
  return `<div class="stack">${yours}${pick ? `<div class="mod-src">${pick}</div>` : ''}${src === 'ts' ? tsCard(a, m, btn) : gbCard(a, m)}</div>`;
}
function tsCard(a, m, btn) {
  const id = a.id, ml = S.modList[id];
  if (!ml) loadModList(id);
  const q = S.modQ.trim().toLowerCase();
  const list = ml && ml.list ? ml.list.filter((p) => !q || [p.name, p.owner, p.desc, ...(p.categories || [])].join(' ').toLowerCase().includes(q)).slice(0, 60) : [];
  return `<div class="card"><div class="sec-h" style="margin-bottom:10px"><h3>${esc(t('From Thunderstore'))}</h3><input class="field" type="search" placeholder="${esc(t('Search mods'))}" value="${esc(S.modQ)}" data-modq aria-label="${esc(t('Search mods'))}"></div>
      ${!ml || ml.loading ? `<p class="muted" style="margin:0">${esc(t('Loading the mods…'))}</p>` : ml.error ? `<p class="err" style="margin:0">${esc(t(ml.error))}</p>`
        : list.length ? `<div class="mods">${list.map((p) => `<div class="mod">${p.icon ? `<img src="${esc(media(p.icon))}" alt="" loading="lazy">` : '<span class="ph"></span>'}<div class="mid"><b>${esc(p.name)}</b><small>${esc(tf('By {who} · {n} downloads · v{v}', { who: p.owner, n: fmtN(p.downloads), v: p.version }))}</small><span>${esc(p.desc)}</span></div><div class="acts"><button type="button" class="btn sm icon" data-act="url" data-url="${esc(p.page)}" title="${esc(t('Its page'))}" aria-label="${esc(t('Its page'))}"><svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${MI.web}</svg></button>${btn(p)}</div></div>`).join('')}</div>` : `<p class="muted" style="margin:0">${esc(t('Nothing matches.'))}</p>`}
      <p class="muted" style="margin:12px 0 0;font-size:13px">${esc(t('Mods come from Thunderstore, made by the community. Most popular first.'))} <a href="#" data-url="https://thunderstore.io/c/${esc(m.community)}/">thunderstore.io</a></p></div>`;
}
// ---------- mods from GameBanana ----------
function gbState(id) {
  if (!S.gb[id]) S.gb[id] = { sort: 'popular', q: '', page: 0, mods: [], total: 0, loading: false, error: '' };
  return S.gb[id];
}
async function gbLoad(id, more) {
  const g = gbState(id);
  if (g.loading) return;
  g.loading = true; g.error = '';
  if (!more) { g.page = 0; g.mods = []; }
  gbRedraw(id);
  const ask = { sort: g.sort, q: g.q };
  try {
    const r = await invoke('gb_mods', { id, sort: g.sort, page: g.page + 1, q: g.q });
    if (ask.sort !== g.sort || ask.q !== g.q) { g.loading = false; return gbLoad(id); }
    g.page += 1;
    const seen = new Set(g.mods.map((x) => x.id));
    g.mods = g.mods.concat(r.mods.filter((x) => !seen.has(x.id)));
    g.total = r.total;
    g.done = !r.mods.length || g.mods.length >= r.total;
  } catch (e) { g.error = String(e && e.message ? e.message : e); }
  g.loading = false;
  gbRedraw(id);
  if (window.tvRefresh) window.tvRefresh();
}
// Redraw the Mods tab without losing a search that's being typed.
function gbRedraw(id) {
  if (!(S.view === 'game' && S.arg === id && S.tab === 'mods')) return;
  const f = document.activeElement, typing = f && f.matches && f.matches('[data-gbq]') ? [f.selectionStart, f.selectionEnd] : null;
  refreshGame();
  if (typing) { const n = $('[data-gbq]'); if (n) { n.focus({ preventScroll: true }); try { n.setSelectionRange(typing[0], typing[1]); } catch (e) { /* fine */ } } }
}
function gbCard(a, m) {
  const id = a.id, g = gbState(id);
  if (!g.mods.length && !g.loading && !g.error && !g.tried) { g.tried = true; gbLoad(id); }
  const have = new Set((m.installed || []).map((x) => x.full));
  const sorts = [['popular', t('Popular')], ['new', t('Newest')], ['liked', t('Most liked')]];
  return `<div class="card"><div class="sec-h" style="margin-bottom:12px"><h3>${esc(t('From GameBanana'))}</h3>
      <div class="gb-ctl"><div class="seg" role="radiogroup" aria-label="${esc(t('Sort'))}">${sorts.map(([k, n]) => `<button type="button" role="radio" aria-checked="${g.sort === k && !g.q}" data-act="gb-sort" data-id="${esc(id)}" data-v="${k}">${esc(n)}</button>`).join('')}</div>
      <input class="field" type="search" placeholder="${esc(t('Search GameBanana'))}" value="${esc(g.q)}" data-gbq="${esc(id)}" aria-label="${esc(t('Search GameBanana'))}"></div></div>
    ${g.error ? `<p class="err" style="margin:0">${esc(t(g.error))} <button type="button" class="btn sm" data-act="gb-retry" data-id="${esc(id)}">${esc(t('Try again'))}</button></p>` : !g.mods.length ? `<p class="muted" style="margin:0">${esc(g.loading ? t('Loading the mods…') : t('Nothing matches.'))}</p>`
      : `<div class="gb-grid">${g.mods.map((x) => `<button type="button" class="gb-mod" data-act="gb-open" data-id="${esc(id)}" data-mod="${x.id}"><span class="gb-img">${x.image ? `<img src="${esc(media(x.image))}" alt="" loading="lazy">` : ''}${have.has('gb:' + x.id) ? `<i class="gb-have">${CHECK}${esc(t('Installed'))}</i>` : ''}</span><b>${esc(x.name)}</b><small>${esc(tf('By {who}', { who: x.author }))}${x.category ? ' · ' + esc(x.category) : ''}</small><span class="gb-likes"><svg width="13" height="13" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M12 21s-7-4.4-9.5-9A5.5 5.5 0 0 1 12 6a5.5 5.5 0 0 1 9.5 6c-2.5 4.6-9.5 9-9.5 9z"/></svg>${esc(fmtN(x.likes))}</span></button>`).join('')}</div>
        ${!g.done ? `<div class="gb-more"><button type="button" class="btn" data-act="gb-more" data-id="${esc(id)}"${g.loading ? ' disabled' : ''}>${esc(g.loading ? t('Loading…') : t('Show more'))}</button></div>` : ''}`}
    <p class="muted" style="margin:14px 0 0;font-size:13px">${esc(t('Mods come from GameBanana, made by the community. Only files that passed GameBanana\'s virus scan can be installed.'))} <a href="#" data-url="https://gamebanana.com/games/${m.gamebanana}">gamebanana.com</a></p></div>`;
}
// Install one of a GameBanana mod's files. A mod that comes in versions asks which, through
// `choose(list)` (a promise of one of them, or null); the download is kept for the second go.
async function gbInstallFile(id, d, fileId, choose) {
  const full = 'gb:' + d.id;
  if (modBusy[full]) return;
  modBusy[full] = 'in'; gbFile[d.id] = fileId;
  if (window.tvRefresh) window.tvRefresh();
  let variant = null;
  try {
    for (;;) {
      try { const rec = await invoke('gb_install', { id, modId: d.id, fileId, variant }); toast(tf('Installed {name}.', { name: rec.name })); break; }
      catch (err) {
        const msg = String(err && err.message ? err.message : err);
        if (!msg.startsWith('variants:') || variant) { fail(err); break; }
        let list = [];
        try { list = JSON.parse(msg.slice(9)); } catch (e) { /* none */ }
        variant = list.length ? await choose(list, d) : null;
        if (!variant) break;
      }
    }
  } finally { delete modBusy[full]; delete modProg[full]; }
  await loadMods(id, true);
  if (window.tvRefresh) window.tvRefresh();
}
window.gbInstallFile = gbInstallFile;
// Which version of a mod, at a desk.
function gbChooseDesk(list, d) {
  return new Promise((resolve) => {
    let picked = null;
    const m = modal(`<h2>${esc(t('Which version?'))}</h2><p class="muted">${esc(tf('{name} comes in versions. Pick the one you want; you can install another later.', { name: d.name }))}</p>
      <div class="gb-variants">${list.map((v, i) => `<button type="button" class="btn" data-x="v" data-i="${i}">${esc(v)}</button>`).join('')}</div>
      <div class="acts"><button type="button" class="btn" data-x="no">${esc(t('Cancel'))}</button></div>`, (x, close) => { if (x.dataset.x === 'v') { picked = list[Number(x.dataset.i)]; close(); } }, t('Which version?'));
    new MutationObserver((_, o) => { if (!m.isConnected) { o.disconnect(); resolve(picked); } }).observe(document.body, { childList: true });
  });
}
// A mod's page: pictures, what it does, and its files to install.
async function gbOpen(id, modId) {
  const m = modal(`<div class="gb-detail"><p class="muted">${esc(t('Loading…'))}</p></div><button type="button" class="sr" data-x="no">${esc(t('Close'))}</button>`, null, t('Mod'));
  m.querySelector('.modal').classList.add('gb-modal');
  let d, img = 0;
  try { d = await invoke('gb_mod', { modId: Number(modId) }); } catch (e) { m.remove(); return fail(e); }
  if (!m.isConnected) return;
  const prog = () => (modProg['gb:' + d.id] ? tf('Installing… {pct}%', { pct: modProg['gb:' + d.id] }) : t('Installing…'));
  const draw = () => {
    const have = (S.mods[id] && S.mods[id] !== 'loading' ? S.mods[id].installed || [] : []).find((x) => x.full === 'gb:' + d.id);
    const busyNow = modBusy['gb:' + d.id];
    const box = m.querySelector('.gb-detail');
    if (!box) return;
    const sr = m.querySelector('.modal > .sr');
    if (sr) sr.remove();
    const had = box.contains(document.activeElement) ? document.activeElement.dataset.x + ':' + (document.activeElement.dataset.file || '') : '';
    box.innerHTML = `<div class="gb-hero">${d.images[img] ? `<img src="${esc(media(d.images[img]))}" alt="">` : ''}</div>
      ${d.images.length > 1 ? `<div class="gb-thumbs">${d.images.slice(0, 6).map((u, i) => `<button type="button" data-gbimg="${i}"${i === img ? ' class="on"' : ''}><img src="${esc(media(u))}" alt=""></button>`).join('')}</div>` : ''}
      <div class="gb-title"><h2>${esc(d.name)}</h2><span>${esc(tf('By {who}', { who: d.author }))} · ${esc(tf('{n} downloads', { n: fmtN(d.downloads) }))}</span></div>
      ${d.text ? `<p class="gb-text">${esc(d.text)}</p>` : ''}
      <h3 class="gb-files-h">${esc(t('Files'))}</h3>
      <div class="gb-files">${d.files.map((f) => `<div class="set-row"><div class="k"><b>${esc(f.name)}</b><span>${esc([f.version ? 'v' + f.version : '', fmtB(f.size), f.desc].filter(Boolean).join(' · '))}${f.ok ? '' : ' · ' + esc(t(f.why))}</span></div>
        ${busyNow && (busyNow !== 'in' || gbFile[d.id] === f.id) ? `<button type="button" class="btn sm${busyNow === 'in' ? ' gb-prog' : ''}" disabled>${esc(busyNow === 'in' ? prog() : t('Removing…'))}</button>`
          : busyNow ? `<button type="button" class="btn sm" disabled>${esc(t('Install'))}</button>`
          : f.ok ? `<button type="button" class="btn sm${have ? '' : ' main'}" data-x="gb-file" data-file="${f.id}"${S.library[id] ? '' : ' disabled'}>${esc(have ? t('Install this one') : t('Install'))}</button>` : `<button type="button" class="btn sm" data-x="gb-page">${esc(t('Get it on GameBanana'))}</button>`}</div>`).join('')}</div>
      <div class="acts" style="margin-top:6px">${have ? `<button type="button" class="btn danger" data-x="gb-remove"${busyNow ? ' disabled' : ''}>${esc(t('Remove'))}</button>` : ''}<button type="button" class="btn" data-x="gb-page">${esc(t('Open on GameBanana'))}</button><button type="button" class="btn" data-x="no">${esc(t('Close'))}</button></div>`;
    // Focus stays where it was, or goes to the first thing to press.
    const back = had && [...box.querySelectorAll('[data-x]')].find((b) => b.dataset.x + ':' + (b.dataset.file || '') === had && !b.disabled);
    if (back) back.focus({ preventScroll: true });
    else if (!m.contains(document.activeElement) || document.activeElement === document.body) { const f = box.querySelector('[data-x="gb-file"]:not([disabled]), [data-x="gb-page"], [data-x="no"]'); if (f) f.focus({ preventScroll: true }); }
  };
  draw();
  // While it installs, only the progress changes.
  const watch = setInterval(() => { if (!m.isConnected) return clearInterval(watch); const b = m.querySelector('.gb-prog'); if (b) b.textContent = prog(); }, 250);
  m.addEventListener('click', async (e) => {
    const im = e.target.closest('[data-gbimg]');
    if (im) { img = Number(im.dataset.gbimg); m.querySelector('.gb-hero').innerHTML = `<img src="${esc(media(d.images[img]))}" alt="">`; m.querySelectorAll('[data-gbimg]').forEach((b) => b.classList.toggle('on', b === im)); return; }
    const x = e.target.closest('[data-x]');
    if (!x) return;
    if (x.dataset.x === 'gb-page') return invoke('open_url', { url: d.page }).catch(fail);
    if (x.dataset.x === 'gb-remove') { if (modBusy['gb:' + d.id]) return; await removeMod(id, 'gb:' + d.id); return draw(); }
    if (x.dataset.x === 'gb-file') {
      const p = gbInstallFile(id, d, Number(x.dataset.file), gbChooseDesk);
      draw();
      await p;
      if (m.isConnected) draw();
    }
  });
}
const modProg = {};
const gbFile = {};
T.event.listen('mod-progress', ({ payload: p }) => { modProg[p.id] = p.total ? Math.min(100, Math.floor((p.done / p.total) * 100)) : 0; });
let gbqTimer;
document.addEventListener('input', (e) => {
  if (!e.target.matches('[data-gbq]')) return;
  const id = e.target.dataset.gbq, g = gbState(id), v = e.target.value;
  clearTimeout(gbqTimer);
  gbqTimer = setTimeout(() => { g.q = v; gbLoad(id); }, 350);
});

async function loadDlHist() {
  try { S.dlHist = (await invoke('download_history')) || []; } catch { S.dlHist = S.dlHist || []; }
  if (S.view === 'downloads') render(true);
}
const took = (s) => (s < 60 ? tf('{n} sec', { n: Math.max(1, s) }) : tf('{n} min', { n: Math.round(s / 60) }));
function pageDownloads() {
  if (!S.dlHist) loadDlHist();
  const hist = S.dlHist || [];
  const act = Object.keys(S.jobs), fails = S.recent.filter((r) => !S.jobs[r.id] && r.phase === 'error');
  const ups = Object.keys(S.updates).filter((id) => S.library[id] && !S.jobs[id]);
  const art = (id) => { const pj = S.jobs[id] && S.jobs[id].plug ? S.jobs[id] : null; if (pj) { const pm = plugMan(pj.plugin); return [{ id, name: pj.name }, `<span class="ph plug-ph">${pm ? plugIcon(pm, pm.icon) : PUZZLE}</span>`]; } const a = byId(id) || { id, name: S.library[id]?.name || id }; return [a, a.cover ? pic(a.cover) : '<span class="ph"></span>']; };
  const total = hist.reduce((n, h) => n + (h.size || 0), 0), secs = hist.reduce((n, h) => n + (h.secs || 0), 0);
  const avg = secs ? total / secs : 0;
  const nInst = Object.values(S.library).filter((g) => !g.local).length;
  const disk = Object.values(S.library).reduce((n, g) => n + (g.size || 0), 0);
  const stats = [
    [t('Downloaded'), fmtB(total) || '0 MB', tn(hist.length, '1 download', '{n} downloads')],
    [t('In your library'), tn(nInst, '1 game', '{n} games'), fmtB(disk) ? tf('{size} downloaded', { size: fmtB(disk) }) : ''],
    [t('Updates installed'), fmtNum(hist.filter((h) => h.update).length), ups.length ? tn(ups.length, '1 more ready', '{n} more ready') : t('All up to date')],
    [t('Average speed'), avg ? tf('{speed}/s', { speed: fmtB(avg) }) : '—', hist[0] ? tf('Last one {when}', { when: ago(hist[0].when) }) : ''],
  ];
  const now = act.map((id) => { const [a, img] = art(id); const j = S.jobs[id] || {};
    return `<div class="dl-now">${img}<div class="mid"><div class="dl-top"><b>${esc(a.name)}</b><span class="tag cy">${esc(j.plug ? ((plugMan(j.plugin) || {}).name || t('Plugin')) : S.library[id] ? t('Update') : t('New'))}</span></div>${progress(id)}</div><button type="button" class="btn" data-act="cancel" data-id="${esc(id)}"${j.phase === 'unpack' ? ' disabled' : ''}>${esc(t('Cancel'))}</button></div>`; }).join('');
  const self = S.launcherUpd ? `<div class="dl-row"><img class="self" src="img/mark.svg" alt=""><div class="mid"><b>RecompLauncher</b><span>${esc(tf('{from} → {to}', { from: verName(S.version), to: verName(S.launcherUpd.version) }))}</span></div><button type="button" class="btn main sm" data-act="self-update">${DOWN}${esc(t('Update'))}</button></div>` : '';
  const upRows = self + ups.map((id) => { const [a, img] = art(id);
    return `<div class="dl-row">${img}<div class="mid"><b>${esc(a.name)}</b><span>${esc(tf('{from} → {to}', { from: S.library[id].tag || '?', to: S.updates[id] }))}</span></div><button type="button" class="btn main sm" data-act="install" data-id="${esc(id)}">${DOWN}${esc(t('Update'))}</button></div>`; }).join('');
  const failRows = fails.map((r) => { const [a, img] = art(r.id);
    return `<div class="dl-row bad">${img}<div class="mid"><b>${esc(a.name)}</b><span class="err">${esc(t(r.msg))}</span></div><button type="button" class="btn sm" data-act="install" data-id="${esc(r.id)}">${esc(t('Try again'))}</button></div>`; }).join('');
  const ui = S.settings.ui || {};
  const shown = hist.filter((h) => (h.when || 0) > (ui.dl_cleared || 0));
  const histRows = shown.slice(0, 40).map((h) => { const [a, img] = art(h.id); const inst = S.library[h.id];
    // The row opens the game's page; Play is its own button.
    return `<div class="dl-row${byId(h.id) || inst ? ' link" data-act="open" data-id="' + esc(h.id) : ''}">${img}<div class="mid"><b>${esc(a.name || h.name)}</b><span>${esc(h.update ? tf('Updated to {tag}', { tag: h.tag }) : tf('Installed {tag}', { tag: h.tag }))}${h.verified ? ' · ' + esc(t('Checksum matched')) : ''}</span></div>
      <span class="dl-meta"><b>${esc(fmtB(h.size))}</b><small>${esc(took(h.secs || 0))}</small></span><span class="dl-when">${esc(agoCap(h.when))}</span>
      ${inst ? `<button type="button" class="icon-btn" data-act="play" data-id="${esc(h.id)}" aria-label="${esc(tf('Play {name}', { name: a.name || h.name }))}" title="${esc(t('Play'))}">${PLAY}</button>` : '<span class="icon-btn" aria-hidden="true"></span>'}</div>`; }).join('');
  const nothing = !act.length && !ups.length && !self && !fails.length && !shown.length;
  const updLog = updateLog(hist).filter((u) => (u.when || 0) > (ui.upd_cleared || 0));
  const tab = S.dlTab === 'updates' ? 'updates' : 'downloads';
  const tabs = `<div class="tabs dl-tabs"><div class="tabs-in" role="tablist"><span class="tab-pill" aria-hidden="true"></span>${[['downloads', t('Downloads')], ['updates', t('Updates')]].map(([k, n]) => `<button type="button" role="tab" class="${tab === k ? 'on' : ''}" aria-selected="${tab === k}" data-act="dl-tab" data-tab="${k}">${esc(n)}</button>`).join('')}</div></div>`;
  // Clearing only hides what's listed; the totals above stay.
  const canClear = tab === 'updates' ? updLog.length : shown.length;
  const bar = `<div class="dl-bar">${tabs}${canClear ? `<button type="button" class="btn sm" data-act="dl-clear" data-k="${tab === 'updates' ? 'upd_cleared' : 'dl_cleared'}">${esc(t('Clear history'))}</button>` : ''}</div>`;
  const head = `<div class="head"><div><h1>${esc(t('Downloads'))}</h1><p>${esc(tab === 'updates' ? t('Every update the launcher has installed, newest first. Open one to read what changed.') : t('Each download is checked against GitHub\'s checksum before it\'s installed.'))}</p></div></div>${bar}`;
  if (tab === 'updates') return `<div class="page dl-page" style="max-width:1100px">${head}${updatesTab(updLog)}</div>`;
  return `<div class="page dl-page" style="max-width:1100px">${head}
    <div class="stats dl-stats">${stats.map(([k, v, sub]) => `<div><span>${esc(k)}</span><b>${esc(v)}</b><small>${esc(sub || '')}</small></div>`).join('')}</div>
    ${act.length ? `<section class="shelf"><div class="shelf-h"><h2>${esc(t('Now'))}</h2></div><div class="list">${now}</div></section>` : ''}
    ${failRows ? `<section class="shelf"><div class="shelf-h"><h2>${esc(t('Didn\'t finish'))}</h2></div><div class="dl-list">${failRows}</div></section>` : ''}
    ${ups.length || self ? `<section class="shelf"><div class="shelf-h"><h2>${esc(t('Updates ready'))}</h2>${ups.length > 1 ? `<button type="button" class="btn sm" data-act="update-all">${DOWN}${esc(t('Update all'))}</button>` : ''}</div><div class="dl-list">${upRows}</div></section>` : ''}
    ${shown.length ? `<section class="shelf"><div class="shelf-h"><h2>${esc(t('History'))}</h2></div><div class="dl-list">${histRows}</div></section>` : ''}
    ${nothing ? `<div class="empty"><h2>${esc(t('No downloads'))}</h2><p>${esc(t('Games you install or update show their progress here.'))}</p></div>` : ''}</div>`;
}

// Downloads > Updates: each update as a row that opens to its patch notes (every release between
// the version it came from and the one it went to), plus the launcher's own updates.
function updateLog(hist) {
  const out = hist.filter((h) => h.update).map((h) => ({ key: `${h.id}@${h.tag}@${h.when}`, id: h.id, name: h.name, tag: h.tag, from: h.from || '', when: h.when, repo: h.repo || (byId(h.id) || {}).repo || '' }));
  for (const u of ((S.settings.ui || {}).self_updates || [])) out.push({ key: `self@${u.to}@${u.when}`, self: true, name: 'RecompLauncher', tag: u.to, from: u.from || '', when: u.when });
  return out.sort((a, b) => (b.when || 0) - (a.when || 0)).slice(0, 80);
}
const relKey = (tag) => String(tag || '').replace(/^v/i, '').toLowerCase();
// The releases an update covers, newest first: from (left out) up to tag.
function relsBetween(list, from, tag) {
  const rel = list.filter((x) => !x.draft);
  const i = rel.findIndex((x) => relKey(x.tag_name) === relKey(tag));
  if (i < 0) return [];
  const j = from ? rel.findIndex((x) => relKey(x.tag_name) === relKey(from)) : -1;
  return rel.slice(i, j > i ? Math.min(j, i + 8) : i + 1);
}
function updNotes(u) {
  if (u.self) {
    const n = S.selfNotes;
    if (n === undefined) { loadSelfNotes(); return `<p class="muted">${esc(t('Loading the notes…'))}</p>`; }
    return n && relKey(n.version) === relKey(u.tag) && n.notes ? `<div class="md">${md(n.notes)}</div>` : `<p class="muted">${esc(t('The notes for this version are on recomplauncher.com.'))}</p>`;
  }
  if (!u.repo) return `<p class="muted">${esc(t('No notes for this one.'))}</p>`;
  const r = S.rels[u.repo];
  if (!r) { loadRels(u.repo); return `<p class="muted">${esc(t('Loading the notes…'))}</p>`; }
  if (r.loading) return `<p class="muted">${esc(t('Loading the notes…'))}</p>`;
  if (r.error) return `<p class="muted">${esc(t(r.error))}</p>`;
  const got = relsBetween(r.list, u.from, u.tag);
  if (!got.length) return `<p class="muted">${esc(t('No notes for this one.'))}</p>`;
  return got.map((x) => `<div class="upd-rel">${got.length > 1 ? `<div class="upd-rel-h"><b>${esc(x.name || x.tag_name)}</b><span>${esc(date(x.published_at))}</span></div>` : ''}<div class="md">${x.body ? md(x.body) : `<p class="muted">${esc(t('No notes for this one.'))}</p>`}</div></div>`).join('');
}
async function loadSelfNotes() {
  if (S.selfNotes === null && loadSelfNotes.busy) return;
  loadSelfNotes.busy = true;
  S.selfNotes = null;
  try { S.selfNotes = await invoke('launcher_notes'); } catch (e) { S.selfNotes = null; }
  loadSelfNotes.busy = false;
  for (const el of $$('.upd.open[data-self]')) fillUpd(el);
}
function fillUpd(el) {
  const u = (S.updLog || []).find((x) => x.key === el.dataset.key);
  const box = el.querySelector('.upd-in');
  if (u && box) box.innerHTML = updNotes(u);
}
function updatesTab(log) {
  S.updLog = log;
  S.updOpen = S.updOpen || new Set();
  if (!log.length) return `<div class="empty"><h2>${esc(t('No updates yet'))}</h2><p>${esc(t('When a game you have gets a new version and you install it, it\'s listed here with what changed.'))}</p></div>`;
  const CHEV = '<svg class="upd-chev" width="16" height="16" viewBox="0 0 16 16" aria-hidden="true"><path d="m4 6 4 4 4-4" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/></svg>';
  return `<div class="upd-list">${log.map((u) => {
    const a = u.self ? null : byId(u.id) || { id: u.id, name: u.name };
    const img = u.self ? '<span class="upd-self" aria-hidden="true"><img src="img/mark.svg" alt=""></span>' : a.cover ? pic(a.cover) : '<span class="ph"></span>';
    const open = S.updOpen.has(u.key);
    return `<div class="upd${open ? ' open' : ''}" data-key="${esc(u.key)}"${u.self ? ' data-self="1"' : ''}>
      <button type="button" class="upd-h" data-act="upd" aria-expanded="${open}">${img}<span class="mid"><b>${esc(u.self ? 'RecompLauncher' : a.name || u.name)}</b><span>${esc(u.from ? tf('{from} → {to}', { from: u.self ? verName(u.from) : u.from, to: u.self ? verName(u.tag) : u.tag }) : tf('Updated to {tag}', { tag: u.self ? verName(u.tag) : u.tag }))}</span></span><span class="dl-when">${esc(agoCap(u.when))}</span>${CHEV}</button>
      <div class="upd-body"><div class="upd-in">${open ? updNotes(u) : ''}</div></div></div>`;
  }).join('')}</div>`;
}
function toggleUpd(el) {
  const row = el.closest('.upd');
  if (!row) return;
  const k = row.dataset.key, open = !row.classList.contains('open');
  if (open) { S.updOpen.add(k); fillUpd(row); } else S.updOpen.delete(k);
  row.classList.toggle('open', open);
  el.setAttribute('aria-expanded', String(open));
}

// ---------- developer tools: a local build and what the launcher makes of its recomplauncher.json ----------
async function devAdd() {
  const p = await pickFile({ title: t('Your recomp\'s folder'), directory: true }).catch(fail);
  if (!p) return;
  try {
    const g = await invoke('dev_add', { path: typeof p === 'string' ? p : p.path });
    S.library[g.id] = g;
    toast(tf('Added {name} as a local build.', { name: g.name }));
    S.nextTab = 'dev';
    go('game', g.id);
  } catch (e) { fail(e); }
}
async function devCheck(id, loud) {
  S.dev[id] = 'loading';
  if (S.view === 'game' && S.arg === id && S.tab === 'dev') showTab('dev');
  try { S.dev[id] = await invoke('dev_check', { id }); } catch (e) { S.dev[id] = { error: String(e && e.message ? e.message : e), items: [], level: 0, source: 'none' }; }
  delete S.setup[id];
  loadSetup(id);
  if (loud) toast(t('Checked it again.'));
  if (S.view === 'game' && S.arg === id && S.tab === 'dev') showTab('dev');
}
const DEV_MARK = { ok: '<path d="m5 12.5 4.5 4.5L19 7.5"/>', warn: '<path d="M12 8v5M12 16.5v.01"/>', err: '<path d="m7 7 10 10M17 7 7 17"/>' };
function devTab(a) {
  const id = a.id, g = S.library[id], r = S.dev[id];
  if (r === undefined) { devCheck(id); return `<div class="stack"><div class="card"><p class="muted" style="margin:0">${esc(t('Checking…'))}</p></div></div>`; }
  if (r === 'loading') return `<div class="stack"><div class="card"><p class="muted" style="margin:0">${esc(t('Checking…'))}</p></div></div>`;
  const levels = [t('Plays from the launcher'), t('Settings in the launcher'), t('Full setup')];
  const src = { folder: t('Your recomplauncher.json'), catalog: t('The file on recomplauncher.com'), 'built-in': t('The file built into the launcher'), none: t('No file yet') }[r.source] || '';
  const errs = r.items.filter((x) => x.level === 'err').length, warns = r.items.filter((x) => x.level === 'warn').length;
  return `<div class="stack">
    <div class="card dev-head"><div class="dev-sum"><span class="kick">${esc(t('Local build'))}</span><h3>${esc(src)}</h3>
      <p class="muted">${esc(r.error ? t('The file has a problem. Players would get none of it.') : errs ? tn(errs, '1 problem to fix.', '{n} problems to fix.') : warns ? tn(warns, 'Works, with 1 thing worth a look.', 'Works, with {n} things worth a look.') : t('Everything checks out.'))}</p></div>
      <div class="acts"><button type="button" class="btn main" data-act="dev-check" data-id="${esc(id)}">${esc(t('Check again'))}</button>${r.has_file ? `<button type="button" class="btn" data-act="dev-file" data-id="${esc(id)}">${esc(t('Open the folder'))}</button>` : `<button type="button" class="btn" data-act="dev-starter" data-id="${esc(id)}">${esc(t('Make a starter file'))}</button>`}</div></div>
    <div class="dev-levels" role="list">${levels.map((n, i) => `<div class="dev-level${r.level > i ? ' on' : ''}" role="listitem"><b>${i + 1}</b><span>${esc(n)}</span></div>`).join('')}</div>
    ${r.error ? `<div class="card dev-err"><h3>${esc(t('It can\'t be read'))}</h3><p class="mono">${esc(r.error)}</p></div>` : ''}
    ${r.items.length ? `<div class="card dev-list">${r.items.map((x) => `<div class="dev-item ${x.level}"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${DEV_MARK[x.level]}</svg><div><b>${esc(x.text)}</b>${x.detail ? `<span>${esc(x.detail)}</span>` : ''}</div></div>`).join('')}</div>` : ''}
    <div class="card"><h3 style="margin-bottom:8px">${esc(t('Try it like a player'))}</h3>
      <p class="muted" style="margin:0 0 14px">${esc(t('Set it up with your own disc, change its settings, then press Play. Change the file and press Check again: the launcher reads it fresh each time.'))}</p>
      <div class="acts"><button type="button" class="btn" data-act="setup" data-id="${esc(id)}">${esc(t('Set up'))}</button><button type="button" class="btn" data-act="tab" data-tab="settings"${S.setup[id] && S.setup[id] !== 'loading' ? '' : ' disabled'}>${esc(t('Settings'))}</button><button type="button" class="btn" data-act="url" data-url="https://recompstore.com/hub?tab=docs">${esc(t('The developer guide'))}</button></div></div>
    <p class="muted dev-path">${esc(g ? g.path : '')}</p>
  </div>`;
}

// Settings: a list of sections down the side, one section at a time beside it.
const SET_ICONS = {
  playing: '<circle cx="12" cy="12" r="9"/><path d="m10 8.5 5 3.5-5 3.5z"/>',
  report: '<path d="M5 21V4.5"/><path d="M5 4.5h11l-2 4 2 4H5"/>',
  account: '<circle cx="12" cy="8.5" r="3.6"/><path d="M4.8 20a7.2 7.2 0 0 1 14.4 0"/>',
  general: '<path d="M4 7h9M17 7h3M4 17h3M11 17h9"/><circle cx="15" cy="7" r="2.2"/><circle cx="9" cy="17" r="2.2"/>',
  library: '<path d="M3 6.5A1.5 1.5 0 0 1 4.5 5H10l2 2.5h7.5A1.5 1.5 0 0 1 21 9v9.5a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 18.5z"/>',
  updates: '<path d="M20 12a8 8 0 1 1-2.3-5.6M20 4v5h-5"/>',
  look: '<circle cx="12" cy="12" r="8.5"/><path d="M12 3.5v17M12 3.5a8.5 8.5 0 0 1 0 17" fill="currentColor"/>',
  bigpicture: '<rect x="2.5" y="4.5" width="19" height="13" rx="2.5"/><path d="M8 21h8"/><path d="m10.5 8.5 4 2.5-4 2.5z" fill="currentColor"/>',
  pads: '<path d="M7 9h10a4 4 0 0 1 3.9 4.9l-.8 3.4a2 2 0 0 1-3.5.8L15 16H9l-1.6 2.1a2 2 0 0 1-3.5-.8l-.8-3.4A4 4 0 0 1 7 9z"/><path d="M8.5 11.5v3M7 13h3"/>',
  pc: '<rect x="3" y="4" width="18" height="12" rx="2"/><path d="M8 20h8M12 16v4"/>',
  dev: '<path d="m8 8-4 4 4 4M16 8l4 4-4 4M13.5 5l-3 14"/>',
  about: '<circle cx="12" cy="12" r="9"/><path d="M12 11v6M12 7.5v.01"/>',
  plugins: '<path d="M10 4.5a2 2 0 0 1 4 0V6h4a1 1 0 0 1 1 1v4h-1.5a2 2 0 0 0 0 4H19v4a1 1 0 0 1-1 1h-4v-1.5a2 2 0 0 0-4 0V20H6a1 1 0 0 1-1-1v-4h1.5a2 2 0 0 0 0-4H5V7a1 1 0 0 1 1-1h4z"/>',
  beta: '<path d="M9.5 3h5M10 3v6.2L4.8 18.1A2 2 0 0 0 6.5 21h11a2 2 0 0 0 1.7-2.9L14 9.2V3"/><path d="M7.2 14h9.6"/>',
};
const SET_SECTIONS = [['general', N_('General')], ['report', N_('Report a Problem')], ['beta', N_('Beta')], ['account', N_('Account')], ['library', N_('Games and folders')], ['updates', N_('Updates and downloads')], ['playing', N_('While Playing')], ['look', N_('Appearance')], ['bigpicture', N_('Big Picture mode')], ['plugins', N_('Plugins')], ['pads', N_('Controllers')], ['pc', N_('Your PC')], ['dev', N_('Developers')], ['about', N_('About')]];
function pageSettings() {
  const st = S.settings, ui = st.ui || {}, sw = (k, on, label, act = 'toggle') => `<button type="button" class="switch" role="switch" aria-checked="${!!on}" aria-label="${esc(label)}" data-act="${act}" data-k="${k}"></button>`;
  const libs = S.libs.length ? S.libs : [{ path: st.library_dir, default: true, games: Object.keys(S.library).length }];
  const pc = S.pc, ok = (v) => okMark(!!v);
  const nOwned = Object.keys(S.owned).length;
  const row = (b, s, ctl) => `<div class="set-row"><div class="k"><b>${esc(b)}</b><span>${esc(s)}</span></div>${ctl}</div>`;
  const group = (title, inner, extra = '') => `<section class="set-group"><div class="set-gh"><h3>${esc(title)}</h3>${extra}</div><div class="card">${inner}</div></section>`;
  const shown = (k) => (k !== 'pc' || S.pc) && (k !== 'dev' || devAllowed());
  const sec = SET_SECTIONS.some(([k]) => k === S.setTab && shown(k)) ? S.setTab : 'general';
  let body = '';
  switch (sec) {
    case 'general':
      if (!S.ids && !S.idsAsked) { S.idsAsked = true; invoke('data_ids').then((ids) => { S.ids = ids; if (S.view === 'settings') render(true); }).catch(() => {}); }
      body = group(t('Language'), row(t('Language'), t('The launcher and its quick menu over your games.'), `<select class="sel" data-act="lang" aria-label="${esc(t('Language'))}">${[['auto', tf('Same as your computer ({lang})', { lang: (LANGS.find((x) => x[0] === systemLang()) || ['', 'English (US)'])[1] })]].concat(LANGS).map(([c, n]) => `<option value="${c}"${(ui.lang || 'auto') === c ? ' selected' : ''}>${esc(n)}</option>`).join('')}</select>`))
        + group(t('Discover'), row(t('Refresh Discover'), t('Get the newest list of recomps, their releases and their art now, instead of waiting for the next check.'), `<button type="button" class="btn" data-act="refresh">${esc(t('Refresh now'))}</button>`)
          + row(t('Hide AI-assisted recomps'), t('Recomps whose developers used AI tools for code, art, writing or voice won\'t show in Discover.'), sw('hide_ai', st.hide_ai, t('Hide AI-assisted recomps'))))
        + (S.os === 'windows' ? group(t('Portable'), portableRow(row)) : S.os === 'linux' ? group(t('Windows recomps'), row(t('Play Windows recomps with Proton'), t('Recomps that only have a Windows version install that one and play through Proton. Steam\'s Proton, GE-Proton, umu-launcher and Wine are found on their own.'), sw('proton', st.proton, t('Play Windows recomps with Proton')))) : '')
        + group(t('Notifications and sharing'), row(t('Notifications'), S.os === 'windows' ? t('A Windows notification when a new recomp comes out for a game you own.') : t('A notification when a new recomp comes out for a game you own.'), sw('notify', st.notify, t('Notifications')))
          + row(t('Discord status'), t('Shows the game you\'re playing on your Discord profile, with its cover and play time.'), sw('discord', st.discord !== false, t('Discord status')))
          + row(t('Share usage and diagnostics'), t('Your PC\'s parts, which games you play and for how long, and what went wrong when one crashes or won\'t install. It helps fix problems faster. No names, no files.'), `<span class="set-ctl2"><a href="#" data-act="url" data-url="https://recompstore.com/privacy#launcher">${esc(t('What\'s sent'))}</a>${sw('share_usage', st.data_asked && st.share_usage, t('Share usage and diagnostics'))}</span>`)
          + row(t('Share anonymous usage stats'), t('When the launcher is open and which recomps are being played, under a random ID that isn\'t linked to you or your account.'), sw('share_stats', st.data_asked && st.share_stats, t('Share anonymous usage stats')))
          + row(t('Your IDs'), S.ids ? tf('Diagnostics {a} · Stats {b}. Quote them to have what was sent removed. A reset starts new ones.', { a: S.ids.usage.slice(0, 8), b: S.ids.stats.slice(0, 8) }) : t('Quote them to have what was sent removed. A reset starts new ones.'), `<span class="set-ctl2"><button type="button" class="btn" data-act="copy-ids">${esc(t('Copy'))}</button><button type="button" class="btn" data-act="reset-ids">${esc(t('Reset'))}</button></span>`))
        + (S.os === 'windows' ? group(t('When you close the window'), row(t('Keep running by the clock'), t('Closing the window leaves RecompLauncher by the clock, ready to start a game. Right-click its icon to quit.'), sw('close_to_tray', st.close_to_tray !== false, t('Keep running by the clock')))) : '');
      break;
    case 'report':
      body = window.reportSection ? window.reportSection(group) : '';
      break;
    case 'beta':
      body = window.betaSection ? window.betaSection(group, row) : '';
      break;
    case 'account':
      body = accountSection(group, row);
      break;
    case 'library':
      body = group(t('Recomp Library'), `<p class="set-note">${esc(t('Each game gets its own folder in a library. Add folders on other drives to spread games out; new games go to the default one unless you pick another when you install.'))}</p>
        ${libs.map((l) => `<div class="set-row"><div class="k"><b class="path">${esc(l.path)}</b><span>${l.free != null ? esc(tf('{free} free', { free: fmtB(l.free) })) + ' · ' : ''}${esc(tn(l.games, '1 game', '{n} games'))}${l.default ? ' · ' + esc(t('default')) : ''}</span></div><div class="acts"><button type="button" class="btn sm" data-act="folder" data-path="${esc(l.path)}">${esc(t('Open'))}</button>${l.default ? '' : `<button type="button" class="btn sm" data-act="lib-default" data-path="${esc(l.path)}">${esc(t('Make default'))}</button><button type="button" class="btn sm danger" data-act="lib-remove" data-path="${esc(l.path)}" data-n="${l.games || 0}">${esc(t('Remove'))}</button>`}</div></div>`).join('')}`,
        `<button type="button" class="btn sm" data-act="lib-add">${esc(t('Add a folder'))}</button>`)
        + group(t('Your games'), `<p class="set-note">${esc(t('Point the launcher at your ROMs, disc images and game backups. It shows which have recomps, sets them up with your copy, and lets you know when a new recomp comes out for one. Files are only read on this PC.'))}</p>
        ${(st.rom_dirs || []).length ? (st.rom_dirs || []).map((d) => `<div class="set-row"><div class="k"><b class="path">${esc(d)}</b><span>${esc(tn(S.roms.filter((r) => String(r.path).startsWith(d)).length, '1 game or disc image', '{n} games and disc images'))}</span></div><div class="acts"><button type="button" class="btn sm" data-act="folder" data-path="${esc(d)}">${esc(t('Open'))}</button><button type="button" class="btn sm danger" data-act="rom-remove" data-path="${esc(d)}">${esc(t('Remove'))}</button></div></div>`).join('') : `<p class="set-empty">${esc(t('No folders yet.'))}</p>`}
        ${nOwned ? `<p class="set-foot">${esc(tn(nOwned, '1 of your games has a recomp.', '{n} of your games have recomps.'))} <a href="#" data-go="home">${esc(t('See them on Home'))}</a></p>` : ''}`,
        `<span class="acts"><button type="button" class="btn sm" data-act="rom-scan">${esc(t('Look again'))}</button><button type="button" class="btn sm main" data-act="rom-add">${esc(t('Add a folder'))}</button></span>`)
        + group(t('Desktop shortcuts'), row(t('Shortcuts Go Through the Launcher'), t('Off: a game\'s desktop shortcut starts the game itself, for front ends like RetroBat. On: it opens the launcher, which brings your Look, cloud saves and play time along.'), sw('shortcut_launcher', ui.shortcut_launcher, t('Shortcuts Go Through the Launcher'), 'ui-toggle')))
        + (S.os === 'windows' ? group(t('Picking files'), row(t('Use the Launcher\'s Own File Browser'), t('Off: Windows\' own window for picking files and folders. On: the launcher\'s, which marks the games it knows and can unzip.'), sw('own_picker', ui.own_picker, t('Use the Launcher\'s Own File Browser'), 'ui-toggle'))) : '')
        + group(t('Recomps you already have'), row(t('Bring them into the Library'), t('Recomps you added to Steam, installed with Quiver Launcher or keep in a folder. They update from here after that, in the same folder.'), `<button type="button" class="btn sm" data-act="adopt">${esc(t('Find them'))}</button>`))
        + group(t('Game Art'), `<div class="set-row"><div class="k"><b>${esc(t('SteamGridDB API Key'))}</b><span>${esc(t('Optional. With your own free key from steamgriddb.com (Preferences → API), games you add from GitHub get covers, backgrounds and logos.'))}</span></div>
          <span class="acts"><input class="field sm" id="sgdb-key" type="password" autocomplete="off" spellcheck="false" value="${esc(ui.sgdb_key || '')}" aria-label="${esc(t('SteamGridDB API Key'))}"><button type="button" class="btn sm" data-act="sgdb-save">${esc(t('Save'))}</button></span></div>`);
      break;
    case 'updates':
      body = group(t('Updates'), row(t('Check for Updates'), t('Asks each installed game\'s GitHub for a newer release now.'), `<button type="button" class="btn sm main" data-act="check-upd">${esc(t('Check now'))}</button>`)
          + row(t('Look for updates when the launcher opens'), t('Asks each installed game\'s GitHub for a newer release. Turn on Update automatically for a game in its Manage tab.'), sw('check_updates', st.check_updates, t('Look for updates')))
          + row(t('Back up saves before updates'), t('Keeps the newest five copies of each game\'s saves and settings.'), sw('backup_before_update', st.backup_before_update, t('Back up before updates'))))
        + group(t('Downloads'), row(t('Keep downloaded files'), t('Off: the zip is deleted once it\'s unpacked. On: it stays in the library\'s .downloads folder.'), sw('keep_downloads', st.keep_downloads, t('Keep downloaded files'))));
      break;
    case 'playing': {
      const hh = !!S.handheld, eff = (k) => (st[k] == null ? hh : !!st[k]);
      body = group(t('While Playing'), (hh ? `<p class="set-note">${esc(t('This PC is a handheld, so the launcher keeps out of the way while you play. Change it here.'))}</p>` : '')
          + row(t('Quick Menu Over Games'), t('Hold View on your controller (or press Ctrl+F12) for screenshots, settings and closing the game. Off: the launcher leaves your controller and keyboard alone while you play.'), sw('quick_menu', st.quick_menu !== false, t('Quick Menu Over Games')))
          + row(t('Minimize the Launcher While Playing'), t('The launcher goes away while a game runs and comes back when it closes.'), sw('minimize_playing', eff('minimize_playing'), t('Minimize the Launcher While Playing')))
          + row(t('Lighter Launcher'), t('Still pictures and no animations, and nothing moves on Home while a game runs. Good for handhelds and older PCs.'), sw('lighter', eff('lighter'), t('Lighter Launcher'))));
      break;
    }
    case 'look':
      body = group(t('Appearance'), `<div class="set-row"><div class="k"><b>${esc(t('Theme'))}</b><span>${esc(t('Dark, black for OLED screens, or light.'))}</span></div><div class="seg" role="radiogroup" aria-label="${esc(t('Theme'))}">${[['dark', t('Dark')], ['oled', t('OLED')], ['light', tc('Light|theme')]].map(([k, n]) => `<button type="button" role="radio" aria-checked="${(ui.theme === 'light' ? 'light' : ui.oled ? 'oled' : 'dark') === k}" data-act="theme" data-k="${k}">${esc(n)}</button>`).join('')}</div></div>`
        + `<div class="set-row"><div class="k"><b>${esc(t('Opens On'))}</b><span>${esc(t('The page you see when the launcher opens.'))}</span></div><div class="seg" role="radiogroup" aria-label="${esc(t('Opens On'))}">${[['', t('Home')], ['library', t('Library')]].map(([k, n]) => `<button type="button" role="radio" aria-checked="${(ui.start_page || '') === k}" data-act="start-page" data-k="${k}">${esc(n)}</button>`).join('')}</div></div>`
        + row(t('Compact sidebar'), t('Only the icons and your pinned games\' covers. The button at the bottom of the sidebar does the same.'), sw('mini', ui.mini, t('Compact sidebar'), 'ui-toggle'))
        + row(t('Cover Size Slider'), t('The slider at the bottom right of the Library, for the size of its covers and to switch to wide tiles.'), sw('hide_tile_ctl', !ui.hide_tile_ctl, t('Cover Size Slider'), 'ui-toggle'))
        + row(t('Covers Only'), t('Hide the names under game covers in the Library and on Home.'), sw('hide_names', ui.hide_names, t('Covers Only'), 'ui-toggle'))
        + row(t('Show Game Names'), t('Name every recomp after its game, like Tears of the Kingdom instead of TotK Recomp. Names you gave games yourself stay.'), sw('game_names', ui.game_names, t('Show Game Names'), 'ui-toggle')))
        + group(t('Sidebar'), `<p class="set-note">${esc(t('Pick which sections the sidebar shows, and their order.'))}</p>` + sideRows(),
          `<button type="button" class="btn sm" data-act="side-reset">${esc(t('Reset'))}</button>`);
      break;
    case 'plugins':
      body = window.pluginSettings ? window.pluginSettings(group, row, sw) : '';
      break;
    case 'bigpicture':
      body = group(t('Big Picture mode'), row(t('Start in Big Picture mode'), t('Opens full screen, ready for a controller. Good for a TV or a handheld.'), sw('start_in_tv', st.start_in_tv, t('Start in Big Picture mode')))
          + `<div class="set-row"><div class="k"><b>${esc(t('Size'))}</b><span>${esc(t('Smaller fits more on a big screen; larger is easier to read from the couch or on a handheld.'))}</span></div>
            <div class="seg" role="radiogroup" aria-label="${esc(t('Big Picture mode size'))}">${[['', t('Auto')], ['small', t('Smaller')], ['medium', t('Medium')], ['large', t('Larger')]].map(([k, n]) => `<button type="button" role="radio" aria-checked="${(st.tv_size || '') === k}" data-act="tvsize" data-k="${k}">${esc(n)}</button>`).join('')}</div></div>`
          + row(t('Sounds'), t('The start-up sound and a soft click as you move around.'), sw('sounds', st.sounds, t('Sounds in Big Picture mode')))
          + (S.steam ? row(t('RecompLauncher in Steam'), S.deck ? t('Puts it in your Steam library, so it\'s there in Gaming Mode and opens in Big Picture mode.') : t('Puts it in your Steam library, opening in Big Picture mode. Handy from Steam\'s own Big Picture.'), `<button type="button" class="btn sm" data-act="self-steam">${esc((st.ui || {}).in_steam ? t('Add it again') : t('Add to Steam'))}</button>`) : ''),
        `<button type="button" class="btn sm main" data-act="tv">${esc(t('Open Big Picture mode'))}</button>`)
        + `<p class="set-tip">${esc(t('Hold Menu on your controller to switch to Big Picture mode.'))}</p>`;
      break;
    case 'pads':
      body = group(t('Controllers'), (S.pads || []).length ? S.pads.map((p) => `<div class="set-row"><div class="k"><b>${esc(p.name)}</b><span>${esc(p.kind === 'ps' ? t('PlayStation controller') : p.kind === 'nintendo' ? t('Nintendo controller') : t('Xbox controller'))}</span></div>${ok(true)}</div>`).join('') : `<p class="set-empty">${esc(t('None connected. Xbox, PlayStation (DualSense and DualShock 4) and Switch Pro controllers work, plugged in or over Bluetooth, and so do 8BitDo pads.'))}</p>`)
        + group(t('Buttons'), `<div class="pad-guide">${buttonGuide().map(([k, n, d]) => `<div><span class="pg-k">${k}</span><span class="pg-t"><b>${esc(n)}</b><small>${esc(d)}</small></span></div>`).join('')}</div>`);
      break;
    case 'pc':
      body = pc ? group(t('Your PC'), `${pc.vcredist != null ? `<div class="set-row"><div class="k"><b>${esc(t('Microsoft Visual C++ runtime'))}</b><span>${esc(t('Almost every Windows recomp needs it.'))}</span></div><div class="acts">${pc.vcredist ? '' : `<button type="button" class="btn sm main" data-act="vcredist">${esc(t('Get it'))}</button>`}${ok(pc.vcredist)}</div></div>` : ''}
        ${pc.vulkan != null ? `<div class="set-row"><div class="k"><b>Vulkan</b><span>${esc(t('Comes with your graphics driver. N64 recomps draw with it.'))}</span></div>${ok(pc.vulkan)}</div>` : ''}
        ${pc.vkbasalt != null ? `<div class="set-row"><div class="k"><b>vkBasalt</b><span>${esc(t('Draws looks over games on Linux. It comes from your distro\'s packages, usually as vkbasalt.'))}</span></div>${ok(pc.vkbasalt)}</div>` : ''}
        ${pc.proton != null ? `<div class="set-row"><div class="k"><b>Proton${pc.proton ? ' · ' + esc(pc.proton) : ''}</b><span>${esc(t('Plays recomps that only have a Windows version. Steam\'s Proton, GE-Proton, umu-launcher or Wine.'))}</span></div>${ok(!!pc.proton)}</div>` : ''}
        <div class="set-row"><div class="k"><b>AVX2</b><span>${esc(t('Some Xbox 360 recomps need a processor from 2013 or later.'))}</span></div>${ok(pc.avx2)}</div>`) : group(t('Your PC'), `<p class="set-empty">${esc(t('Loading…'))}</p>`);
      break;
    case 'dev': {
      const locals = Object.values(S.library).filter((g) => g.local && g.tag !== 'own');
      const me = devAccount();
      body = (me ? `<div class="dev-me">${me.avatar ? `<img src="${esc(me.avatar)}" alt="" loading="lazy">` : ''}<span><b>${esc(me.name || me.slug)} ${TICK(15)}</b><small>${esc(t('Signed in as a verified developer'))}</small></span></div>` : '')
        + group(t('Developer tools'), row(t('Developer tools'), t('For recomp developers: add a build from a folder on this PC and check its recomplauncher.json before you ship it.'), sw('dev', ui.dev, t('Developer tools'), 'ui-toggle'))
          + (ui.dev ? row(t('Add a build from a folder'), t('Your recomp\'s folder, with its program and recomplauncher.json. It shows in your Library as a local build; the launcher never updates, moves or deletes it.'), `<button type="button" class="btn main" data-act="dev-add">${esc(t('Add a build'))}</button>`) : ''))
        + (ui.dev ? group(t('Your local builds'), locals.length ? locals.map((g) => `<div class="set-row"><div class="k"><b>${esc(g.name)}</b><span class="path">${esc(g.path)}</span></div><div class="acts"><button type="button" class="btn sm" data-act="dev-open" data-id="${esc(g.id)}">${esc(t('Check it'))}</button></div></div>`).join('') : `<p class="set-empty">${esc(t('None yet.'))}</p>`) : '')
        + group(t('The developer guide'), row(t('Works with RecompLauncher'), t('How to write recomplauncher.json: Easy Setup from the player\'s disc, your settings in the launcher, and your own setup step.'), `<button type="button" class="btn" data-act="url" data-url="https://recompstore.com/hub?tab=docs">${esc(t('Open the guide'))}</button>`));
      break;
    }
    default:
      body = group(t('About'), `<p style="margin:0 0 8px">RecompLauncher ${esc(verName(S.version))} · ${OS_NAME[S.os]} ${esc(S.arch)}</p>
        <p class="muted" style="margin:0">${esc(t('RecompLauncher has no game files and never downloads any. Every recomp comes from its developer\'s own GitHub, and you set it up with your own copy of the game.'))}</p>`, `<button type="button" class="btn sm" data-act="about">${esc(t('More about it'))}</button>`)
        + group(t('Support'), row(t('Support RecompLauncher'), t('Help keep it running and growing, on Ko-fi'), `<button type="button" class="btn" data-act="url" data-url="${KOFI}">${HEART}${esc(t('Support Us'))}</button>`))
        + (discordUrl() ? group(t('Community'), row(t('Discord'), t('News, help with a game, and a place to send your logs.'), `<button type="button" class="btn" data-act="url" data-url="${esc(discordUrl())}">${DISCORD}${esc(t('Join the Discord'))}</button>`)) : '')
        + group(t('Start over'), row(t('Reset settings'), t('Every setting goes back to how it started. Your games, saves, library folders and pinned games stay.'), `<button type="button" class="btn" data-act="reset" data-k="settings">${esc(t('Reset settings'))}</button>`)
          + row(t('First-time setup'), t('Go through first-time setup again. Nothing is deleted.'), `<button type="button" class="btn" data-act="reset" data-k="setup">${esc(t('Run it again'))}</button>`));
  }
  return `<div class="page set-page"><div class="head"><div><h1>${esc(t('Settings'))}</h1></div></div>
    <div class="set-wrap"><nav class="set-nav" aria-label="${esc(t('Settings'))}">${SET_SECTIONS.filter(([k]) => shown(k)).map(([k, n]) => `<button type="button" class="set-tab${k === sec ? ' on' : ''}" data-act="set-tab" data-k="${k}"${k === sec ? ' aria-current="page"' : ''}><svg width="19" height="19" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${SET_ICONS[k]}</svg><span>${esc(t(n))}</span></button>`).join('')}</nav>
    <div class="set-body" data-sec="${sec}">${body}</div></div></div>`;
}

// ---------- drawing ----------
// ---------- redrawing without flicker ----------
// A page redraw (an update finishing, a setting toggled, a download ticking) patches what's on
// screen into the new markup instead of replacing it: what didn't change stays as it is, so
// covers don't reload, entrance animations don't play again and the scroll stays put.
function morph(el, html) {
  const tpl = document.createElement('template');
  tpl.innerHTML = html;
  morphKids(el, tpl.content);
}
// A child's identity among its siblings: its own key, id, or what it opens.
function nodeKey(n) {
  if (n.nodeType !== 1) return '';
  const k = n.getAttribute('data-key') || n.id;
  if (k) return k;
  const id = n.getAttribute('data-id');
  return id ? (n.getAttribute('data-act') || n.getAttribute('data-tvact') || n.tagName) + ':' + id : '';
}
function sameKind(a, b) {
  return a.nodeType === b.nodeType && (a.nodeType !== 1 || a.tagName === b.tagName);
}
function morphKids(parent, next) {
  const news = Array.from(next.childNodes);
  // Keys that are unique on both sides match children even when they've moved.
  const count = (list) => { const m = new Map(); for (const c of list) { const k = nodeKey(c); if (k) m.set(k, (m.get(k) || 0) + 1); } return m; };
  const nc = count(news), oc = count(parent.childNodes);
  const uniq = (k) => k && nc.get(k) === 1 && oc.get(k) === 1;
  const byKey = new Map();
  for (const c of parent.childNodes) { const k = nodeKey(c); if (uniq(k)) byKey.set(k, c); }
  for (let i = 0; i < news.length; i++) {
    const n = news[i], cur = parent.childNodes[i] || null, k = nodeKey(n);
    let m = null;
    if (uniq(k)) { m = byKey.get(k); byKey.delete(k); if (m && !sameKind(m, n)) m = null; }
    else if (cur && !uniq(nodeKey(cur)) && sameKind(cur, n)) m = cur;
    if (m) {
      if (m !== cur) parent.insertBefore(m, cur);
      morphNode(m, n);
    } else {
      parent.insertBefore(n, cur);
    }
  }
  while (parent.childNodes.length > news.length) parent.removeChild(parent.lastChild);
}
function morphNode(o, n) {
  if (o.nodeType !== 1) {
    if (o.nodeValue !== n.nodeValue) o.nodeValue = n.nodeValue;
    return;
  }
  for (const a of Array.from(o.attributes)) {
    // Inline styles the layout code sets after a draw (grid columns, title sizes) stay until it sets them again.
    if (!n.hasAttribute(a.name) && a.name !== 'style') o.removeAttribute(a.name);
  }
  for (const a of Array.from(n.attributes)) if (o.getAttribute(a.name) !== a.value) o.setAttribute(a.name, a.value);
  const tag = o.tagName;
  if (tag === 'INPUT') {
    if (o.type === 'checkbox' || o.type === 'radio') o.checked = n.hasAttribute('checked');
    else if (document.activeElement !== o) o.value = n.getAttribute('value') || '';
    return;
  }
  if (tag === 'TEXTAREA') {
    if (document.activeElement !== o) o.value = n.textContent;
    return;
  }
  if (tag === 'VIDEO' || tag === 'CANVAS' || tag === 'IFRAME') return;
  morphKids(o, n);
  if (tag === 'SELECT') {
    const i = Array.from(n.options).findIndex((x) => x.hasAttribute('selected'));
    o.selectedIndex = i < 0 ? 0 : i;
  }
}
function scrollAnchor(main) {
  const mr = main.getBoundingClientRect();
  for (const el of $$('.tile[data-id], .dl-row[data-id]', main)) {
    const r = el.getBoundingClientRect();
    if (r.bottom > mr.top + 60 && r.top < mr.bottom && r.width) {
      const id = el.dataset.id;
      return { id, n: $$(`[data-id="${CSS.escape(id)}"]`, main).indexOf(el), y: r.top };
    }
  }
  return null;
}
function render(keepScroll) {
  const main = $('#main'), top = main.scrollTop;
  const v = S.view;
  // Typing in a search: it keeps its text, caret and focus across a redraw.
  const ae = document.activeElement, typing = ae && (ae.id === 'dq' || ae.id === 'lq') ? { id: ae.id, v: ae.value, a: ae.selectionStart, b: ae.selectionEnd } : null;
  if (typing) { if (typing.id === 'dq') S.q = typing.v; else S.lq = typing.v; }
  const gp = $('.gp', main);
  // A plugin's page keeps running while the rest of the window is redrawn.
  if (v.startsWith('plug:') && !S.anim && main.dataset.v === v) { renderSide(); paintIsland(); $('#offline').hidden = !S.offline; return; }
  if (keepScroll && v === 'game' && !S.anim && gp && gp.dataset.gp === S.arg) { refreshGame(); renderSide(); paintIsland(); $('#offline').hidden = !S.offline; fitTitles(main); return; }
  const html = v === 'discover' ? pageDiscover() : v === 'library' ? pageLibrary() : v === 'achievements' ? pageAchievements() : v === 'game' ? pageGame(S.arg) : v === 'downloads' ? pageDownloads() : v === 'settings' ? pageSettings() : v === 'shots' ? pageShots() : v === 'saves' && window.pageSaves ? window.pageSaves() : v.startsWith('plug:') && window.pagePlugin ? window.pagePlugin(v.slice(5)) : pageHome();
  // Going to a page draws it fresh, with its entrance. Anything else (the same page with new
  // data) is patched in place, so nothing on screen flickers.
  // Redrawing the same page: whatever was at the top of the view stays there, even when something
  // above it came or went (a row, a badge, a banner).
  const anchor = keepScroll && !S.anim && main.dataset.v === v ? scrollAnchor(main) : null;
  if (S.anim || main.dataset.v !== v) main.innerHTML = html;
  else morph(main, html);
  main.dataset.v = v;
  if (keepScroll) main.scrollTop = top;
  if (anchor) { const el = $$(`[data-id="${CSS.escape(anchor.id)}"]`, main)[anchor.n]; if (el) main.scrollTop += el.getBoundingClientRect().top - anchor.y; }
  if (S.anim) {
    main.classList.remove('enter', 'enter-game', 'enter-soft', 'enter-back');
    void main.offsetWidth;
    main.classList.add(S.anim);
    let i = 0;
    for (const el of $$('.grid > .tile, .shotgrid > .shot', main)) el.style.setProperty('--i', Math.min(i++, 18));
    S.anim = null;
  }
  renderSide();
  paintIsland();
  $('#offline').hidden = !S.offline;
  evenGrids(main);
  layoutShelves(main);
  fitTitles(main);
  moveTabPill();
  if (v === 'game') paintLooks();
  if (typing) { const f = $('#' + typing.id); if (f) { f.focus({ preventScroll: true }); try { f.setSelectionRange(typing.a, typing.b); } catch (e) { /* fine */ } } }
}
// The sidebar's sections in the player's order, with the ones they hid. Home and Library always show.
const SIDE_KEEP = ['home', 'library'];
function sideSections() {
  const ui = S.settings.ui || {}, order = ui.side_order || [];
  const all = $$('#side .navs .nav[data-go]');
  all.forEach((b, i) => { if (b.dataset.i == null) b.dataset.i = i; });
  const rank = (b) => { const i = order.indexOf(b.dataset.go); return i >= 0 ? i : 100 + Number(b.dataset.i); };
  return all.sort((a, b) => rank(a) - rank(b));
}
function applySide() {
  const ui = S.settings.ui || {}, hide = new Set(ui.side_hide || []);
  const nav = $('#side .navs'), list = sideSections();
  if (list.some((b, i) => nav.children[i] !== b)) list.forEach((b) => nav.appendChild(b));
  for (const b of list) b.hidden = hide.has(b.dataset.go) && !SIDE_KEEP.includes(b.dataset.go);
}
function sideRows() {
  const hide = new Set((S.settings.ui || {}).side_hide || []), list = sideSections();
  return list.map((b, i) => {
    const k = b.dataset.go, name = $('.lbl', b).textContent, keep = SIDE_KEEP.includes(k);
    const icon = ($('svg, img', b) || {}).outerHTML || '';
    return `<div class="set-row side-row"><div class="k"><b><span class="side-ic">${icon}</span>${esc(name)}</b></div><span class="acts">
      <button type="button" class="icon-btn" data-act="side-move" data-k="${esc(k)}" data-d="-1" ${i === 0 ? 'disabled' : ''} aria-label="${esc(tf('Move {name} up', { name }))}"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="m6 15 6-6 6 6"/></svg></button>
      <button type="button" class="icon-btn" data-act="side-move" data-k="${esc(k)}" data-d="1" ${i === list.length - 1 ? 'disabled' : ''} aria-label="${esc(tf('Move {name} down', { name }))}"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="m6 9 6 6 6-6"/></svg></button>
      <button type="button" class="switch" role="switch" aria-checked="${keep || !hide.has(k)}" ${keep ? 'disabled title="' + esc(t('Always shown')) + '"' : ''} aria-label="${esc(tf('Show {name}', { name }))}" data-act="side-show" data-k="${esc(k)}"></button></span></div>`;
  }).join('');
}

// The sidebar: which page is on, counts, and the pinned games.
function renderSide() {
  const v = S.view, ui = S.settings.ui || {};
  applySide();
  for (const b of $$('.side .nav[data-go]')) b.classList.toggle('on', b.dataset.go === (v === 'game' ? (pins().includes(S.arg) ? '' : S.from) : v));
  // Downloading now, or else how many updates are ready (games and the launcher), as a badge.
  const nd = Object.keys(S.jobs).length;
  const nu = Object.keys(S.updates || {}).filter((id) => S.library[id] && !S.jobs[id]).length + (S.launcherUpd ? 1 : 0);
  const nb = $('#n-dl');
  nb.textContent = nd || nu || '';
  nb.classList.toggle('badge-n', !nd && nu > 0);
  nb.title = !nd && nu ? tn(nu, '1 update ready', '{n} updates ready') : '';
  document.body.classList.toggle('oled', !!ui.oled && ui.theme !== 'light');
  document.body.classList.toggle('nonames', !!ui.hide_names);
  document.body.classList.toggle('light', ui.theme === 'light');
  { const wm = $('#brand .wm'); const want = ui.theme === 'light' ? 'img/wordmark-dark.svg' : 'img/wordmark.svg'; if (wm && wm.getAttribute('src') !== want) wm.setAttribute('src', want); }
  const auto = innerWidth < 1180;
  $('#side').classList.toggle('auto', auto);
  document.body.classList.toggle('mini-side', !!ui.mini || auto);
  $('#side').classList.toggle('mini', !!ui.mini);
  // Names show as tooltips only when the sidebar shows icons alone.
  const iconsOnly = !!ui.mini || auto;
  for (const b of $$('.side .nav, .side .pin')) { const l = $('.lbl', b); if (l) { if (iconsOnly) b.title = l.textContent; else b.removeAttribute('title'); } }
  $('#collapse').title = ui.mini ? t('Expand') : t('Collapse');
  $('#collapse .lbl').textContent = ui.mini ? t('Expand') : t('Collapse');
  const list = pins().map((id) => byId(id) || (S.library[id] && { id, name: S.library[id].name })).filter(Boolean);
  $('#pins-h').hidden = !list.length;
  const html = list.map((a) => `<button type="button" class="pin${v === 'game' && S.arg === a.id ? ' on' : ''}" data-act="open" data-id="${esc(a.id)}" title="${esc(a.name)}"><span class="th">${a.cover ? pic(a.cover) : ''}${S.running.has(a.id) ? '<i class="run"></i>' : ''}</span><span class="lbl">${esc(a.name)}</span></button>`).join('');
  const box = $('#pins');
  if (box.dataset.html !== html) { box.innerHTML = html; box.dataset.html = html; }
  for (const b of $$('.pin', box)) if (!iconsOnly) b.removeAttribute('title');
}
// The island at the bottom: what's downloading, always in view.
function paintIsland() {
  const el = $('#isl');
  if (!el) return;
  const ids = Object.keys(S.jobs);
  const off = !ids.length || S.view === 'downloads' || document.body.classList.contains('tv-on');
  document.body.classList.toggle('isl-on', !off);
  if (off) { el.hidden = true; el.dataset.id = ''; return; }
  const id = ids[0], j = S.jobs[id], a = (j && j.plug && { name: j.name }) || byId(id) || { name: (S.library[id] || {}).name || id };
  const [l, r] = progLabel(j);
  const busy = !j || j.phase !== 'download' || !j.total;
  if (el.hidden || el.dataset.id !== id) {
    el.hidden = false; el.dataset.id = id;
    el.innerHTML = `<span class="th">${a.cover ? pic(a.cover) : ''}</span><span class="k"><b></b><small></small></span><span class="pc"></span><i class="ln"></i>`;
  }
  $('b', el).textContent = S.library[id] && !(j && j.plug) ? tf('Updating {name}', { name: a.name }) : tf('Installing {name}', { name: a.name });
  $('small', el).textContent = (ids.length > 1 ? tf('{n} more after it', { n: ids.length - 1 }) + ' · ' : '') + [l, r].filter(Boolean).join(' · ');
  $('.pc', el).textContent = busy ? '' : pct(j) + '%';
  const ln = $('.ln', el);
  ln.classList.toggle('busy', busy);
  ln.style.width = pct(j) + '%';
  el.setAttribute('aria-label', tf('{what}. Open Downloads.', { what: $('b', el).textContent }));
}

// Grids never end on a lone tile: pick a column count (one more or one fewer than fits) that
// leaves at least two on the last row.
function evenGrids(scope) {
  for (const g of $$('.grid, .shotgrid', scope)) {
    const n = g.children.length, min = g.closest('#llist') ? (Number(getComputedStyle(g.closest('#llist')).getPropertyValue('--tile')) || libTile()) * (g.closest('#llist.wide') ? 1.7 : 1) : g.classList.contains('shotgrid') ? 260 : g.classList.contains('compact') ? 140 : 160;
    const gap = parseFloat(getComputedStyle(g).columnGap) || 20, w = g.clientWidth;
    if (!w) continue;
    const fits = Math.max(1, Math.floor((w + gap) / (min + gap)));
    let cols = fits;
    if (n > fits) cols = [fits, fits - 1, fits + 1, fits - 2].find((c) => c >= 2 && n % c !== 1) || fits;
    else if (n > 1) cols = fits;
    g.style.gridTemplateColumns = `repeat(${cols}, minmax(0, 1fr))`;
  }
}

function movePill() { moveTabPill(); }

// ---------- smooth changes ----------
// Only transform and opacity move, so it stays smooth on any screen, 60 Hz or 240 Hz.
const EASE = 'cubic-bezier(.2,.8,.2,1)';
const calm = () => matchMedia('(prefers-reduced-motion: reduce)').matches;
const onScreen = (r) => r.bottom > -60 && r.top < innerHeight + 60 && r.width > 0;
// Where each element is now, so glideFrom can move it from there to where it ends up.
const spots = (els, rel) => { const o = rel ? rel.getBoundingClientRect() : { left: 0, top: 0 }; const m = new Map(); for (const el of els) { const r = el.getBoundingClientRect(); if (onScreen(r)) m.set(el, { x: r.left - o.left, y: r.top - o.top, w: r.width }); } return m; };
function glideFrom(before, rel, ms = 300) {
  if (calm()) return;
  const o = rel ? rel.getBoundingClientRect() : { left: 0, top: 0 };
  for (const [el, a] of before) {
    if (!el.isConnected) continue;
    const r = el.getBoundingClientRect();
    const dx = a.x - (r.left - o.left), dy = a.y - (r.top - o.top), k = r.width ? a.w / r.width : 1;
    if (Math.abs(dx) < 0.5 && Math.abs(dy) < 0.5 && Math.abs(k - 1) < 0.005) continue;
    el.animate([{ transform: `translate(${dx}px, ${dy}px) scale(${k})`, transformOrigin: '0 0' }, { transform: 'none', transformOrigin: '0 0' }], { duration: ms, easing: EASE });
  }
}
// A list of tiles drawn again (a filter, a sort, a search): the tiles that stay are the same
// elements, so their pictures never reload, and they glide to their new places; new ones fade in.
function morphList(box, html) {
  const oldGrid = $('.grid', box);
  const tpl = document.createElement('template');
  tpl.innerHTML = html;
  const newGrid = tpl.content.querySelector('.grid');
  const fresh = [];
  if (oldGrid && newGrid && oldGrid.className === newGrid.className) {
    const old = new Map($$(':scope > .tile[data-id]', oldGrid).map((el) => [el.dataset.id, el]));
    const before = spots(old.values());
    const kids = [...newGrid.children].map((n) => {
      const o = old.get(n.dataset.id);
      if (o && (o.__sig === undefined || o.__sig === n.innerHTML)) return o;
      n.__sig = n.innerHTML;
      fresh.push(n);
      return n;
    });
    // The rest of the box (the count) comes from the new markup; the grid keeps its elements.
    const keep = oldGrid;
    keep.replaceChildren(...kids);
    newGrid.replaceWith(keep);
    box.replaceChildren(...tpl.content.childNodes);
    evenGrids(box);
    fitTitles(box);
    glideFrom(before);
  } else {
    box.replaceChildren(...tpl.content.childNodes);
    fresh.push(...$$('.grid > .tile', box));
    evenGrids(box);
    fitTitles(box);
  }
  if (calm()) return;
  let i = 0;
  for (const el of fresh) {
    if (!onScreen(el.getBoundingClientRect())) continue;
    el.animate([{ opacity: 0, transform: 'translateY(8px) scale(.98)' }, { opacity: 1, transform: 'none' }], { duration: 280, easing: EASE, delay: Math.min(i++, 10) * 14, fill: 'backwards' });
  }
}
// Discover's sort, filters and toggles: the controls change in place, the list morphs.
function updateDiscover() {
  if (S.view !== 'discover' || !$('#dlist')) return render(true);
  const f = S.find;
  for (const b of $$('.dctl [data-act="sort"]')) b.setAttribute('aria-checked', String(b.dataset.k === f.sort));
  const gr = $('.dctl [data-act="great"]');
  if (gr) gr.setAttribute('aria-checked', String(!!f.great));
  const ez = $('.dctl [data-act="easy-f"]');
  if (ez) ez.setAttribute('aria-checked', String(!!f.easy));
  const mp = $('.dctl [data-act="mp-f"]');
  if (mp) mp.setAttribute('aria-checked', String(!!f.mp));
  const ai = $('.dctl [data-k="hide_ai"]');
  if (ai) ai.setAttribute('aria-checked', String(!!S.settings.hide_ai));
  const h = $('.dhead h1');
  if (h) h.textContent = S.q.trim() ? t('Search') : f.genre ? t(GENRES[f.genre]) : t('Discover');
  if (window.syncDrops) window.syncDrops();
  morphList($('#dlist'), discoverGrid());
}
function updateLibrary() {
  if (S.view !== 'library' || !$('#llist')) return render(true);
  const was = S.lib.filter;
  libFilterOk();
  if (was !== S.lib.filter) return render(true);
  for (const c of $$('.chips [data-act="libf"]')) c.classList.toggle('on', c.dataset.f === S.lib.filter);
  if (window.syncDrops) window.syncDrops();
  morphList($('#llist'), libraryGrid());
}
// The sidebar folds and opens with the page sliding along, instead of the page being drawn again.
function toggleSide() {
  const mini = !(S.settings.ui || {}).mini;
  const main = $('#main'), side = $('#side');
  S.settings.ui = Object.assign({}, S.settings.ui || {}, { mini });
  saveUi({ mini }, true);
  // The page settles once, after the sidebar is done: games glide to their new spots.
  const settle = () => {
    side.classList.remove('sliding');
    document.body.classList.remove('sliding');
    const before = spots($$('.grid > .tile, .shelf .track > *', main), main);
    evenGrids(main);
    layoutShelves(main);
    fitTitles(main);
    moveTabPill();
    if (!calm()) glideFrom(before, main, 260);
  };
  if (calm()) { renderSide(); return settle(); }
  // The sidebar itself slides to its new width, and the page follows its edge.
  clearTimeout(toggleSide.t);
  // While it slides, the grids keep their columns at the size they are, so a frame only moves
  // the page instead of laying out every game again; they settle into their new size at the end.
  for (const g of $$('.grid', main)) {
    const c = g.firstElementChild;
    const n = (getComputedStyle(g).gridTemplateColumns || '').split(' ').filter(Boolean).length;
    if (c && n) g.style.gridTemplateColumns = `repeat(${n}, ${c.getBoundingClientRect().width}px)`;
  }
  document.body.classList.add('sliding');
  side.classList.add('sliding');
  void side.offsetWidth;
  renderSide();
  const gen = (toggleSide.gen = (toggleSide.gen || 0) + 1);
  const done = (e) => { if (e && (e.target !== side || e.propertyName !== 'width')) return; side.removeEventListener('transitionend', done); if (gen !== toggleSide.gen) return; clearTimeout(toggleSide.t); settle(); };
  side.addEventListener('transitionend', done);
  toggleSide.t = setTimeout(done, 420);
}

function go(view, arg, fromEl) {
  if (view === 'settings') { loadLibs(); loadPads(); }
  if (view === 'shots') loadAllShots();
  if (view === 'saves' && window.loadSavesAll) window.loadSavesAll();
  if (view === S.view && arg === S.arg) { if (view === 'game' && S.nextTab) { S.tab = S.nextTab; S.nextTab = null; } return render(true); }
  S.back.push({ view: S.view, arg: S.arg, tab: S.tab, scroll: $('#main').scrollTop });
  if (S.back.length > 40) S.back.shift();
  if (view === 'game') { S.from = S.view === 'game' ? S.from : S.view; S.tab = S.nextTab || 'overview'; S.nextTab = null; }
  const swap = () => {
    S.view = view; S.arg = arg;
    S.anim = view === 'game' ? 'enter-game' : 'enter';
    render();
    $('#main').scrollTop = 0;
  };
  // A soft cross-fade between pages, where the window can do it (WebKitGTK on Linux can crash on it).
  if (S.os === 'windows' && document.startViewTransition && !matchMedia('(prefers-reduced-motion: reduce)').matches) { document.startViewTransition(swap); return; }
  swap();
  void fromEl;
}

function back() {
  if (back.busy) return;
  const b = S.back.pop();
  if (!b) return go('home');
  const main = $('#main'), cur = main.firstElementChild;
  const swap = () => {
    back.busy = false;
    S.view = b.view; S.arg = b.arg; S.tab = b.tab || 'overview';
    S.anim = 'enter-back';
    render();
    if (b.scroll) main.scrollTop = b.scroll;
  };
  if (calm() || !cur) return swap();
  // The page you're leaving sinks and fades, then the one you came from settles back in.
  back.busy = true;
  const out = cur.animate([{ opacity: 1, transform: 'none' }, { opacity: 0, transform: 'translateY(16px) scale(.985)' }], { duration: 160, easing: 'cubic-bezier(.4,0,1,1)', fill: 'forwards' });
  out.onfinish = swap;
  out.oncancel = swap;
}

// Only the progress bars move during a download; the rest of the page stays put.
function paintProgress(id) {
  const j = S.jobs[id];
  for (const el of $$(`[data-prog="${CSS.escape(id)}"]`)) {
    const [l, r] = progLabel(j), tr = $('.track', el);
    tr.classList.toggle('busy', !j || j.phase !== 'download' || !j.total);
    $('i', tr).style.width = pct(j) + '%';
    const s = $$('.lbl span', el); s[0].textContent = l; s[1].textContent = r;
  }
  for (const el of $$(`[data-prog-bar="${CSS.escape(id)}"]`)) el.style.width = pct(j) + '%';
}

// ---------- talking to the core ----------
async function refreshState() {
  const st = await invoke('get_state');
  Object.assign(S, { os: st.os, arch: st.arch, version: st.version, site: st.site, library: st.library || {}, settings: st.settings || {}, roms: st.roms || [], steam: !!st.steam, deck: !!st.deck, bigArg: !!st.bigArg, appimage: !!st.appimage });
  S.running = new Set(st.running || []);
  S.missing = new Set(st.missing || []);
  for (const id of Object.keys(S.library)) byId(id);
  // The core's list of jobs is the truth: one it no longer has is over (unless it was only just
  // started here), and one that just ended here isn't brought back by an answer sent a moment before.
  const live = new Set(st.jobs || []), now = Date.now();
  for (const id of Object.keys(S.jobs)) if (!live.has(id) && now - (S.jobs[id].at || 0) > 5000) delete S.jobs[id];
  for (const id of live) if (!S.jobs[id] && !(now - (ENDED[id] || 0) < 5000)) S.jobs[id] = { id, phase: 'prepare', done: 0, total: 0, at: now };
}
const ENDED = {};

async function loadCatalog(force) {
  try {
    const r = await invoke('get_catalog', { force: !!force });
    S.cat = Object.assign({ apps: [], origins: [] }, r.data);
    S.offline = !!r.offline;
    S.owned = ownedHere(S.owned);
    // Keep every cover and banner on disk, so scrolling never waits and offline still looks right.
    const urls = uniq(S.cat.apps.flatMap((a) => [a.cover, a.hero, a.logo]).filter(Boolean).map(webUrl)).filter((u) => /^https?:/.test(u));
    invoke('prefetch_images', { urls }).catch(() => {});
    // The list says there's no version for this system: check the releases themselves.
    // Only for entries that don't say which systems they're for: the list's own answer is trusted.
    const ask = S.cat.apps.filter((a) => a.repo && !a.soon && !(a.systems || []).length && !(a.assets && Object.keys(a.assets).length)).map((a) => a.id);
    if (ask.length) invoke('build_check', { ids: ask }).then((b) => { S.builds = Object.assign({}, S.builds, b); if (Object.values(b).some(Boolean)) { render(true); if (window.tvRefresh) window.tvRefresh(); } }).catch(() => {});
  } catch (e) {
    if (!S.cat) S.cat = { apps: [], origins: [] };
    S.offline = true;
    fail(e);
  }
}

async function loadRels(repo, force) {
  if (!repo || (S.rels[repo] && S.rels[repo].loading)) return;
  S.rels[repo] = { loading: true, list: [] };
  try {
    const r = await invoke('get_releases', { repo, force: !!force });
    S.rels[repo] = { list: Array.isArray(r.data) ? r.data : [] };
  } catch (e) {
    S.rels[repo] = { list: [], error: String(e) };
  }
  if (S.view === 'game') refreshGame();
  else if (S.view === 'downloads') for (const el of $$('.upd.open')) fillUpd(el);
}

async function checkUpdates(loud) {
  try { S.updates = await invoke('check_updates'); } catch (e) { if (loud) fail(e); return false; }
  renderSide();
  if (window.tvRefresh && document.body.classList.contains('tv-on')) window.tvRefresh();
  if (S.view === 'downloads') render(true);
  const auto = Object.keys(S.updates).filter((id) => S.library[id] && S.library[id].auto_update && !S.running.has(id) && !S.jobs[id]);
  for (const id of auto) startInstall(id);
  const n = Object.keys(S.updates).length - auto.length;
  if (auto.length) toast(auto.length === 1 ? tf('Updating {name}.', { name: S.library[auto[0]].name }) : tf('Updating {n} games.', { n: auto.length }));
  if (n > 0) {
    const one = n === 1 && S.library[Object.keys(S.updates).find((k) => !auto.includes(k))]?.name;
    toast(n === 1 ? (one ? tf('An update is ready for {name}.', { name: one }) : t('An update is ready for a game.')) : tf('Updates are ready for {n} games.', { n }));
  }
  if (['home', 'library', 'game'].includes(S.view)) render(true);
  return true;
}

// A new language: saved, then everything drawn again in it.
async function setLanguage(code) {
  const ui = Object.assign({}, S.settings.ui || {}, { lang: code });
  const next = Object.assign({}, S.settings, { ui });
  try { await invoke('save_settings', { settings: next }); S.settings = next; } catch (e) { return fail(e); }
  await loadLang(code === 'auto' ? systemLang() : code);
  tDom();
  render(true);
  trayWords();
  if (window.tvRefresh) window.tvRefresh();
}
window.setLanguage = setLanguage;
// Another section of Settings: only the right side changes, with a short fade. The list on the
// left and the page around it stay as they are.
function setSection(k, fade = true) {
  if (S.view !== 'settings') { S.setTab = k; return go('settings'); }
  S.setTab = k;
  const ob = $('#main .set-body');
  if (!ob) return render(true);
  const tmp = document.createElement('div');
  tmp.innerHTML = pageSettings();
  const nb = tmp.querySelector('.set-body');
  if (!nb) return render(true);
  // The same section with new values (a pad plugged in): patched in place, without a flicker.
  if (k === ob.dataset.sec) morphNode(ob, nb);
  else {
    if (fade) nb.classList.add('swap');
    ob.replaceWith(nb);
  }
  for (const b of $$('#main .set-tab')) { const on = b.dataset.k === k; b.classList.toggle('on', on); if (on) b.setAttribute('aria-current', 'page'); else b.removeAttribute('aria-current'); }
  if (k === 'pads' && fade) loadPads();
  const main = $('#main'), wrap = $('.set-wrap', main);
  if (fade && wrap && main.scrollTop > wrap.offsetTop - 20) main.scrollTop = Math.max(0, wrap.offsetTop - 20);
}
// One save at a time, each with everything changed so far: two quick toggles both stick.
let settingsSaving = Promise.resolve();
// `quiet`: the caller has already shown the change itself, so the page isn't drawn again.
function saveSettings(patch, quiet) {
  S.settings = Object.assign({}, S.settings, patch);
  settingsSaving = settingsSaving.then(() => invoke('save_settings', { settings: S.settings })).catch((e) => fail(e)).then(() => { if (!quiet) render(true); });
  return settingsSaving;
}

// ---------- picking a file or folder: the launcher's own browser ----------
// pickFile({title, directory, filters, defaultPath}) resolves to a path, or null when cancelled.
// TV mode draws its own (tv.js); the system's picker is still one click away.
function systemPicker(opts) {
  if (T.dialog && T.dialog.open) return T.dialog.open(opts);
  return invoke('plugin:dialog|open', { options: opts });
}
const FB_ICONS = {
  folder: '<path d="M3 6.5A1.5 1.5 0 0 1 4.5 5H10l2 2.5h7.5A1.5 1.5 0 0 1 21 9v9.5a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 18.5z"/>',
  game: '<path d="M3 6.5A1.5 1.5 0 0 1 4.5 5H10l2 2.5h7.5A1.5 1.5 0 0 1 21 9v9.5a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 18.5z"/><circle cx="12" cy="14" r="2.6"/>',
  disc: '<circle cx="12" cy="12" r="9"/><circle cx="12" cy="12" r="2.5"/>',
  rom: '<path d="M6 3h12v14l-3 4H6z"/><path d="M9 7h6M9 11h6"/>',
  program: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 9h18"/>',
  file: '<path d="M6 3h8l4 4v14H6z"/><path d="M14 3v4h4"/>',
  drive: '<rect x="3" y="7" width="18" height="10" rx="2"/><circle cx="17" cy="12" r="1"/>',
  desktop: '<rect x="3" y="4" width="18" height="12" rx="2"/><path d="M8 20h8M12 16v4"/>',
  documents: '<path d="M6 3h8l4 4v14H6z"/><path d="M9 12h6M9 16h6"/>',
  downloads: '<path d="M12 4v11M7 10l5 5 5-5M5 20h14"/>',
  home: '<path d="M4 11l8-7 8 7v9H4z"/><path d="M10 20v-6h4v6"/>',
  games: '<circle cx="12" cy="12" r="9"/><circle cx="12" cy="12" r="2.5"/>',
  library: '<rect x="4" y="4" width="6.5" height="6.5" rx="1.5"/><rect x="13.5" y="4" width="6.5" height="6.5" rx="1.5"/><rect x="4" y="13.5" width="6.5" height="6.5" rx="1.5"/><rect x="13.5" y="13.5" width="6.5" height="6.5" rx="1.5"/>',
  up: '<path d="M12 19V5M6 11l6-6 6 6"/>',
};
Object.assign(FB_ICONS, {
  refresh: '<path d="M20 11a8 8 0 1 0-2.3 5.7M20 4v7h-7"/>',
  pin: '<path d="M9 3.5h6l-1 5.5 4 3v2H6v-2l4-3z"/><path d="M12 14v6.5"/>',
  zip: '<path d="M6 3h9l4 4v14H6z"/><path d="M11 5h2M11 8h2M11 11h2M10.5 14h3v4h-3z"/>',
});
const fbIcon = (k, size = 20) => `<svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${FB_ICONS[k] || FB_ICONS.file}</svg>`;
// The files a picker wants: by extension, or anything when it says '*'.
// Archives show in every picker, to be extracted on the spot.
const FB_ARCHIVE = /\.(zip|7z|rar)$/i;
function fbAccepts(opts, e) {
  if (!e.dir && FB_ARCHIVE.test(e.name)) return true;
  return fbTakes(opts, e);
}
// What can be picked (an archive only when the picker asks for one).
function fbTakes(opts, e) {
  if (e.dir) return true;
  if (opts.directory) return false;
  const exts = (opts.filters || []).flatMap((f) => f.extensions || []);
  if (!exts.length || exts.includes('*')) return true;
  const x = (e.name.split('.').pop() || '').toLowerCase();
  return exts.includes(x);
}
// Where a picker starts: what it was given, else where this kind of picker was last, else the
// first game folder, else Downloads.
const FB_LAST = {};
function fbStart(opts, places) {
  const key = opts.directory ? 'dir' : 'file';
  let last = FB_LAST[key];
  try { last = last || localStorage.getItem('fb-' + key); } catch (e) { /* fine */ }
  return opts.defaultPath || last || (places.games[0] || {}).path || (places.quick.find((q) => q.kind === 'downloads') || places.quick[0] || places.drives[0] || {}).path || '';
}
function fbRemember(opts, dir) {
  const key = opts.directory ? 'dir' : 'file';
  FB_LAST[key] = dir;
  try { localStorage.setItem('fb-' + key, dir); } catch (e) { /* fine */ }
}
// The pieces of a path, for the clickable trail: C:\Games\PS2 -> C:\, Games, PS2.
function fbCrumbs(path) {
  // A network share, \\server\share\folder: the share is the first piece.
  const unc = /^\\\\([^\\]+)\\([^\\]+)\\?(.*)$/.exec(path);
  if (unc) {
    let acc = `\\\\${unc[1]}\\${unc[2]}\\`;
    const out = [{ name: `\\\\${unc[1]}\\${unc[2]}`, path: acc }];
    unc[3].split(/\\+/).filter(Boolean).forEach((p) => { acc += (acc.endsWith('\\') ? '' : '\\') + p; out.push({ name: p, path: acc }); });
    return out;
  }
  const win = /^[a-zA-Z]:/.test(path) || path.includes('\\');
  const sep = win ? '\\' : '/';
  const parts = path.split(/[\\/]+/).filter(Boolean);
  const out = [];
  let acc = win ? '' : '/';
  if (!win) out.push({ name: '/', path: '/' });
  parts.forEach((p, i) => {
    acc = win ? (i === 0 ? p + sep : acc + (acc.endsWith(sep) ? '' : sep) + p) : acc + (acc.endsWith('/') ? '' : '/') + p;
    out.push({ name: i === 0 && win ? p : p, path: acc });
  });
  return out;
}
// Lighter Launcher: no animations or moving pictures (CSS does the rest).
function applyLighter() {
  const st = S.settings || {};
  document.body.classList.toggle('lighter', st.lighter == null ? !!S.handheld : !!st.lighter);
}
async function pickFile(opts) {
  if (document.body.classList.contains('tv-on') && window.tvBrowse) return window.tvBrowse(opts);
  // On the desktop, Windows' own picker unless the player chose the launcher's in Settings: it
  // opens at once, reaches every folder and takes a pasted path.
  if (!(S.settings.ui || {}).own_picker && S.os === 'windows' && !opts.launcherPicker) {
    try {
      const last = FB_LAST[opts.directory ? 'dir' : 'file'];
      const p = await systemPicker(Object.assign({}, opts, { defaultPath: opts.defaultPath || last || undefined }));
      if (p) fbRemember(opts, opts.directory ? (typeof p === 'string' ? p : p.path) : String(typeof p === 'string' ? p : p.path).replace(/[\\/][^\\/]*$/, ''));
      return p;
    } catch (e) { /* the launcher's own picker instead */ }
  }
  return fbOpen(opts);
}
const FB = { el: null };
// The places the core names (Desktop, Documents, Downloads, Home) are shown through t().
const FB_PLACES = [N_('Desktop'), N_('Documents'), N_('Downloads'), N_('Home')];
void FB_PLACES;
async function fbOpen(opts) {
  // A second click while it's opening does nothing; one that's open already is closed first.
  if (FB.opening) return null;
  if (FB.el && FB.done) FB.done(null);
  FB.opening = true;
  let places;
  // Asking every drive takes a moment: what was found stays for a minute.
  try {
    if (FB.placesAt && Date.now() - FB.placesAt < 60000) places = FB.placesCache;
    else { places = await invoke('fs_places'); FB.placesCache = places; FB.placesAt = Date.now(); }
  } catch (e) { FB.opening = false; return systemPicker(opts); }
  FB.opening = false;
  return new Promise((resolve) => {
    const s = document.createElement('div');
    s.className = 'scrim';
    s.innerHTML = `<div class="modal fb" role="dialog" aria-modal="true" aria-label="${esc(opts.title || t('Choose'))}"></div>`;
    document.body.append(s);
    Object.assign(FB, { el: s, opts, places, path: '', list: null, games: {}, sel: -1, q: '', resolve, busy: 0 });
    const done = (v) => { if (!FB.el) return; FB.el.remove(); FB.el = null; resolve(v); };
    FB.done = done;
    // Closed by a click outside it only when the press started outside too: a drag that ends
    // past its edge (selecting a path, scrolling) doesn't close it.
    let downOut = false;
    s.addEventListener('mousedown', (e) => { downOut = e.target === s; });
    s.addEventListener('click', (e) => {
      if (e.target === s) { if (downOut) done(null); return; }
      const b = e.target.closest('[data-fb]');
      if (!b) return;
      const k = b.dataset.fb;
      if (k === 'go') return fbGo(b.dataset.path);
      if (k === 'refresh') return fbRefresh();
      if (k === 'pin') return fbTogglePin(FB.path);
      if (k === 'unpin') { e.stopPropagation(); return fbTogglePin(b.dataset.path); }
      if (k === 'extract') return fbExtract();
      if (k === 'up') return FB.list && FB.list.parent && fbGo(FB.list.parent);
      if (k === 'row') { fbSelect(Number(b.dataset.i)); if (e.detail >= 2) fbActivate(); return; }
      if (k === 'pick') return fbPick();
      if (k === 'cancel') return done(null);
      if (k === 'system') {
        FB.el.remove(); FB.el = null;
        systemPicker(opts).then((p) => resolve(typeof p === 'string' ? p : p && p.path ? p.path : null)).catch((err) => { fail(err); resolve(null); });
      }
    });
    s.addEventListener('keydown', fbKeys);
    fbDraw(true);
    fbGo(fbStart(opts, places));
  });
}
function fbRows() {
  const L = FB.list;
  if (!L) return [];
  const q = FB.q.trim().toLowerCase();
  return L.entries.filter((e) => fbAccepts(FB.opts, e) && (!q || e.name.toLowerCase().includes(q)));
}
async function fbGo(path) {
  if (!path || !FB.el) return;
  const n = ++FB.busy;
  FB.loading = true;
  fbDraw();
  let v;
  try { v = await invoke('fs_list', { path }); } catch (e) { v = { path, parent: null, entries: [], denied: true, games: {} }; }
  if (n !== FB.busy || !FB.el) return;
  if (!v) v = { path, parent: null, entries: [], denied: true, games: {} };
  FB.list = v; FB.games = v.games || {}; FB.path = v.path; FB.sel = -1; FB.q = ''; FB.loading = false;
  fbDraw();
  const list = $('.fb-list', FB.el);
  if (list) list.scrollTop = 0;
  const f = $('.fb-row', FB.el) || $('.fb-pick', FB.el);
  if (f) f.focus({ preventScroll: true });
}
function fbSelect(i) {
  FB.sel = i;
  $$('.fb-row', FB.el).forEach((r) => r.classList.toggle('on', Number(r.dataset.i) === i));
  const r = $(`.fb-row[data-i="${i}"]`, FB.el);
  if (r) { r.focus({ preventScroll: true }); r.scrollIntoView({ block: 'nearest' }); }
  fbFoot();
}
function fbActivate() {
  const e = fbRows()[FB.sel];
  if (!e) return;
  if (e.dir) return fbGo(e.path);
  if (!fbTakes(FB.opts, e) && FB_ARCHIVE.test(e.name)) return fbExtract();
  fbPick();
}
// The file that's selected, or for a folder picker the selected folder (or the one that's open).
function fbChoice() {
  const e = fbRows()[FB.sel];
  if (FB.opts.directory) return e && e.dir ? e.path : FB.path;
  return e && !e.dir && fbTakes(FB.opts, e) ? e.path : '';
}
function fbPick() {
  const p = fbChoice();
  if (!p) return;
  fbRemember(FB.opts, FB.opts.directory ? p : FB.path);
  FB.done(p);
}
function fbFoot() {
  const foot = $('.fb-foot', FB.el);
  if (!foot) return;
  const p = fbChoice(), g = p && FB.games[p];
  $('.fb-chosen', foot).innerHTML = p ? `${g ? `<span class="fb-game">${DISC_S}${esc(g.name)}</span>` : ''}<span class="fb-path">${esc(p)}</span>` : `<span class="fb-path muted">${esc(FB.opts.directory ? t('Open a folder, or select one') : t('Select a file'))}</span>`;
  $('.fb-pick', foot).disabled = !p;
  const sel = fbRows()[FB.sel];
  const x = $('.fb-x', foot);
  if (x) { x.hidden = !(sel && !sel.dir && /\.(zip|7z|rar)$/i.test(sel.name)); x.disabled = !!FB.extracting; }
}
// Pinned folders, at the top of the side: kept with the launcher's settings.
const fbPins = () => ((S.settings.ui || {}).fb_pins || []).filter((p) => typeof p === 'string');
function fbPinsHtml() {
  const pins = fbPins();
  if (!pins.length) return '';
  const nameOf = (p) => p.replace(/[\\/]+$/, '').split(/[\\/]/).pop() || p;
  return `<div class="fb-sec"><h4>${esc(t('Pinned'))}</h4>${pins.map((p) => `<div class="fb-pinrow"><button type="button" class="fb-place${FB.path === p ? ' on' : ''}" data-fb="go" data-path="${esc(p)}" title="${esc(p)}">${fbIcon('pin')}<span class="n">${esc(nameOf(p))}</span></button><button type="button" class="fb-unpin" data-fb="unpin" data-path="${esc(p)}" aria-label="${esc(t('Unpin this folder'))}" title="${esc(t('Unpin this folder'))}">${XMARK}</button></div>`).join('')}</div>`;
}
function fbTogglePin(path) {
  if (!path) return;
  const pins = fbPins();
  const next = pins.includes(path) ? pins.filter((p) => p !== path) : [path].concat(pins).slice(0, 12);
  saveUi({ fb_pins: next }, true);
  const box = $('.fb-pins', FB.el);
  if (box) box.innerHTML = fbPinsHtml();
  fbDraw();
  toast(next.includes(path) ? t('Pinned. It\'s at the top of the side now.') : t('Unpinned.'));
}
// The folder again, as it is now (a file just copied in shows up), keeping what was selected.
async function fbRefresh() {
  const keep = (fbRows()[FB.sel] || {}).path;
  await fbGo(FB.path);
  if (keep) { const i = fbRows().findIndex((r) => r.path === keep); if (i >= 0) fbSelect(i); }
}
// A zip, 7z or rar unpacked next to itself, into a folder named after it; then that folder opens.
async function fbExtract() {
  const sel = fbRows()[FB.sel];
  if (!sel || FB.extracting) return;
  FB.extracting = true; fbFoot();
  const x = $('.fb-x', FB.el);
  if (x) x.innerHTML = `<span class="spinr" style="width:14px;height:14px"></span>${esc(t('Extracting…'))}`;
  try {
    const dest = await invoke('fs_extract', { path: sel.path });
    FB.extracting = false;
    toast(t('Extracted.'));
    if (FB.el) await fbGo(dest);
  } catch (e) {
    FB.extracting = false;
    fail(e);
    if (FB.el) { if (x) x.innerHTML = `${fbIcon('zip', 15)}${esc(t('Extract here'))}`; fbFoot(); }
  }
}
function fbDraw(first) {
  const box = $('.modal', FB.el);
  if (!box) return;
  const P = FB.places, here = FB.path;
  const side = (title, list) => (list.length ? `<div class="fb-sec"><h4>${esc(title)}</h4>${list.map((p) => `<button type="button" class="fb-place${here === p.path ? ' on' : ''}" data-fb="go" data-path="${esc(p.path)}" title="${esc(p.path)}">${fbIcon(p.kind)}<span class="n">${esc(p.kind === 'drive' ? tf('Drive {name}', { name: p.name }) : ['desktop', 'documents', 'downloads', 'home'].includes(p.kind) ? t(p.name) : p.name)}</span>${p.kind === 'drive' && p.total ? `<i class="fb-space" title="${esc(tf('{free} free of {total}', { free: fmtB(p.free), total: fmtB(p.total) }))}"><u style="width:${Math.round(100 - (p.free / p.total) * 100)}%"></u></i>` : ''}</button>`).join('')}</div>` : '');
  if (first) {
    box.innerHTML = `<div class="fb-head"><h2>${esc(FB.opts.title || t('Choose'))}</h2><button type="button" class="su-x" data-fb="cancel" aria-label="${esc(t('Close'))}">${XMARK}</button></div>
      <div class="fb-in"><aside class="fb-side"><div class="fb-pins">${fbPinsHtml()}</div>${side(t('Your game folders'), P.games)}${side(t('Quick access'), P.quick)}${side(t('Drives'), P.drives)}${side(t('Game library'), P.library)}</aside>
      <div class="fb-main"><div class="fb-bar"><button type="button" class="btn sm icon" data-fb="up" aria-label="${esc(t('Up a folder'))}" title="${esc(t('Up a folder'))}">${fbIcon('up', 16)}</button><button type="button" class="btn sm icon" data-fb="refresh" aria-label="${esc(t('Refresh'))}" title="${esc(t('Refresh'))} (F5)">${fbIcon('refresh', 16)}</button><div class="fb-trail"></div><button type="button" class="btn sm icon fb-pinbtn" data-fb="pin" aria-pressed="false" aria-label="${esc(t('Pin this folder'))}" title="${esc(t('Pin this folder'))}">${fbIcon('pin', 16)}</button>
      <label class="fb-q"><span class="sr">${esc(t('Filter'))}</span>${SEARCH.replace('width="20" height="20"', 'width="15" height="15"')}<input type="search" placeholder="${esc(t('Filter, or paste a path'))}" autocomplete="off" spellcheck="false"></label></div>
      <div class="fb-list" role="listbox"></div>
      <div class="fb-foot"><div class="fb-chosen"></div><button type="button" class="btn sm fb-x" data-fb="extract" hidden>${fbIcon('zip', 15)}${esc(t('Extract here'))}</button><button type="button" class="btn sm" data-fb="system">${esc(t('System picker'))}</button><button type="button" class="btn" data-fb="cancel">${esc(t('Cancel'))}</button><button type="button" class="btn main fb-pick" data-fb="pick" disabled>${esc(FB.opts.directory ? t('Use this folder') : t('Choose'))}</button></div></div></div>`;
    $('.fb-q input', box).addEventListener('input', (e) => { FB.q = /^([a-z]:[\\/]|\\\\|\/)/i.test(e.target.value) ? '' : e.target.value; FB.sel = -1; fbDrawList(); fbFoot(); });
    // A path typed or pasted there (C:\Games, \\server\share) opens with Enter.
    $('.fb-q input', box).addEventListener('keydown', (e) => {
      const v = e.target.value.trim().replace(/^"|"$/g, '');
      if (e.key === 'Enter' && /^([a-z]:[\\/]?|\\\\|\/)/i.test(v)) { e.preventDefault(); e.stopPropagation(); e.target.value = ''; FB.q = ''; fbGo(/^[a-z]:$/i.test(v) ? v + '\\' : v); }
    });
  }
  for (const b of $$('.fb-place', box)) b.classList.toggle('on', b.dataset.path === here);
  const pinned = fbPins().includes(here);
  const pb = $('.fb-pinbtn', box);
  if (pb) { pb.setAttribute('aria-pressed', String(pinned)); pb.disabled = !here; pb.title = pinned ? t('Unpin this folder') : t('Pin this folder'); pb.setAttribute('aria-label', pb.title); }
  const trail = $('.fb-trail', box);
  trail.innerHTML = here ? fbCrumbs(here).map((c, i, a) => `<button type="button" class="fb-crumb${i === a.length - 1 ? ' on' : ''}" data-fb="go" data-path="${esc(c.path)}">${esc(c.name)}</button>`).join('<span class="fb-sep">›</span>') : '';
  trail.scrollLeft = trail.scrollWidth;
  $('[data-fb="up"]', box).disabled = !(FB.list && FB.list.parent);
  fbDrawList();
  fbFoot();
}
function fbDrawList() {
  const list = $('.fb-list', FB.el);
  if (!list) return;
  if (FB.loading && !FB.list) { list.innerHTML = `<p class="fb-empty">${esc(t('Loading…'))}</p>`; return; }
  const L = FB.list;
  if (L && L.denied) { list.innerHTML = `<p class="fb-empty">${esc(t('This folder can\'t be opened.'))}</p>`; return; }
  const rows = fbRows();
  if (!rows.length) { list.innerHTML = `<p class="fb-empty">${esc(FB.q ? t('Nothing matches.') : FB.opts.directory ? t('No folders in here. Use this one, or go up.') : t('Nothing you can pick in here.'))}</p>`; return; }
  list.innerHTML = rows.map((e, i) => {
    const g = FB.games[e.path];
    const kind = g && !e.dir ? 'disc' : e.kind;
    return `<button type="button" class="fb-row${g ? ' has' : ''}${i === FB.sel ? ' on' : ''}" role="option" data-fb="row" data-i="${i}">${fbIcon(kind === 'game' ? 'game' : kind)}<span class="n">${esc(e.name)}</span>${g ? `<span class="fb-game">${DISC_S}${esc(g.name)}</span>` : '<span></span>'}<span class="fb-meta">${e.dir ? '' : esc(fmtB(e.size))}</span></button>`;
  }).join('') + (L && L.more ? `<p class="fb-empty">${esc(t('Only the first 2,000 items are shown. Type to filter.'))}</p>` : '');
}
// A controller in the file browser: up and down move, A opens or picks, B goes up a folder (and
// closes at the top), X picks the folder that's open, Y switches to the places on the side.
window.fbPad = (b) => {
  if (!FB.el) return false;
  const rows = fbRows();
  const inSide = document.activeElement && document.activeElement.closest('.fb-side');
  if (inSide && ['up', 'down'].includes(b)) {
    const ps = $$('.fb-place', FB.el), i = ps.indexOf(document.activeElement);
    const n = ps[Math.max(0, Math.min(ps.length - 1, i + (b === 'down' ? 1 : -1)))];
    if (n) n.focus();
    return true;
  }
  if (b === 'up' || b === 'down') { if (rows.length) fbSelect(Math.max(0, Math.min(rows.length - 1, FB.sel + (b === 'down' ? 1 : -1)))); }
  else if (b === 'left' || b === 'y') { const p = $('.fb-place.on', FB.el) || $('.fb-place', FB.el); if (p) p.focus(); }
  else if (b === 'right') { if (rows.length) fbSelect(Math.max(0, FB.sel)); }
  else if (b === 'a') {
    const r = document.activeElement && document.activeElement.closest('.fb-row');
    if (inSide) document.activeElement.click(); else if (r) { fbSelect(Number(r.dataset.i)); fbActivate(); } else if (FB.sel >= 0) fbActivate(); else fbPick();
  }
  else if (b === 'x') fbPick();
  else if (b === 'b') { if (FB.list && FB.list.parent && !inSide) fbGo(FB.list.parent); else FB.done(null); }
  return true;
};
function fbKeys(e) {
  if (!FB.el) return;
  const typing = e.target.matches('input');
  const rows = fbRows();
  if (e.key === 'Escape') { e.preventDefault(); return FB.done(null); }
  if (e.key === 'F5' || (e.key === 'r' && (e.ctrlKey || e.metaKey))) { e.preventDefault(); return fbRefresh(); }
  if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
    e.preventDefault();
    if (!rows.length) return;
    fbSelect(Math.max(0, Math.min(rows.length - 1, FB.sel + (e.key === 'ArrowDown' ? 1 : -1))));
  } else if (e.key === 'Enter' && !e.target.closest('[data-fb]:not(.fb-row)')) {
    e.preventDefault();
    const r = e.target.closest('.fb-row');
    if (r) { fbSelect(Number(r.dataset.i)); fbActivate(); } else if (FB.sel >= 0) fbActivate(); else fbPick();
  } else if (e.key === 'Backspace' && !typing) {
    e.preventDefault();
    if (FB.list && FB.list.parent) fbGo(FB.list.parent);
  } else if (!typing && e.key.length === 1 && /\S/.test(e.key) && !e.ctrlKey && !e.metaKey && !e.altKey) {
    // Typing jumps to the first name that starts that way.
    const i = rows.findIndex((r) => r.name.toLowerCase().startsWith(e.key.toLowerCase()));
    if (i >= 0) fbSelect(i);
  }
}

// ---------- menus and confirmations ----------
function closeMenu() { $$('.menu').forEach((m) => m.remove()); }
const MI = {
  plus: '<path d="M12 5v14M5 12h14"/>',
  play: '<path d="M7 4.8v14.4a.8.8 0 0 0 1.2.7l11.5-7.2a.8.8 0 0 0 0-1.4L8.2 4.1A.8.8 0 0 0 7 4.8z" fill="currentColor" stroke="none"/>',
  down: '<path d="M12 4v11m0 0-4.5-4.5M12 15l4.5-4.5M5 20h14"/>',
  page: '<rect x="4" y="4" width="16" height="16" rx="3"/><path d="M8 9h8M8 13h8M8 17h5"/>',
  desk: '<rect x="3" y="4" width="18" height="13" rx="2"/><path d="M8 21h8M12 17v4"/>',
  steam: '<path d="M12 5v14M5 12h14"/>',
  folder: '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>',
  shot: '<rect x="3" y="6" width="18" height="14" rx="2.5"/><path d="M8.5 6l1.5-2h4l1.5 2"/><circle cx="12" cy="13" r="3.5"/>',
  save: '<path d="M5 4h11l3 3v13H5z"/><path d="M8 4v5h7V4M8 20v-6h8v6"/>',
  move: '<path d="M4 12h14m0 0-5-5m5 5-5 5"/>',
  exe: '<path d="m8 9-4 3 4 3M16 9l4 3-4 3M13.5 6l-3 12"/>',
  web: '<circle cx="12" cy="12" r="8.5"/><path d="M3.5 12h17M12 3.5c2.5 2.6 2.5 14.4 0 17M12 3.5c-2.5 2.6-2.5 14.4 0 17"/>',
  bin: '<path d="M5 7h14M10 7V5h4v2M7 7l1 13h8l1-13"/>',
  info: '<circle cx="12" cy="12" r="8.5"/><path d="M12 11v6M12 7.5h.01"/>',
  gear: '<circle cx="12" cy="12" r="3"/><path d="M12 3v2.5M12 18.5V21M3 12h2.5M18.5 12H21M5.6 5.6l1.8 1.8M16.6 16.6l1.8 1.8M5.6 18.4l1.8-1.8M16.6 7.4l1.8-1.8"/>',
  refresh: '<path d="M20 11a8 8 0 1 0-2.3 5.7M20 4v7h-7"/>',
  stop: '<path d="m6 6 12 12M18 6 6 18"/>',
  share: '<path d="M12 15V4m0 0-4 4m4-4 4 4"/><path d="M5 12v6a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-6"/>',
  puzzle: '<path d="M9 4h4v2.5a1.5 1.5 0 0 0 3 0V4h4v6h-2.5a1.5 1.5 0 0 0 0 3H20v7h-6v-2.5a1.5 1.5 0 0 0-3 0V20H4v-6h2.5a1.5 1.5 0 0 0 0-3H4V4z"/>',
  look: '<path d="M12 3.5l1.9 5.1 5.1 1.9-5.1 1.9L12 17.5l-1.9-5.1L5 10.5l5.1-1.9z"/><path d="M18.5 15.5l.8 2 2 .8-2 .8-.8 2-.8-2-2-.8 2-.8z"/>',
  history: '<path d="M4 12a8 8 0 1 0 2.3-5.6M4 4v4.5h4.5"/><path d="M12 8v4.5l3 2"/>',
  pin: '<path d="M9 3.5h6l-1 5.5 4 3v2H6v-2l4-3z"/><path d="M12 14v6.5"/>',
  star: '<path d="m12 3.5 2.6 5.3 5.9.9-4.3 4.1 1 5.8L12 16.9l-5.2 2.7 1-5.8-4.3-4.1 5.9-.9z"/>',
  disc: '<circle cx="12" cy="12" r="9"/><circle cx="12" cy="12" r="2.5"/>',
  layers: '<path d="M12 3 3 8l9 5 9-5-9-5Z"/><path d="m3 13 9 5 9-5"/>',
};
// A menu at a button or the pointer. An item with `sub` opens its group in the same spot, with a
// Back row at the top, so the menu stays short.
const M_CHEV = '<svg class="chev" width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 6 6 6-6 6"/></svg>';
function menuRows(items) {
  return items.map((i, n) => {
    if (i === '-') return '<hr>';
    const icon = i.icon ? `<svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${MI[i.icon] || ''}</svg>` : '';
    if (i.sub) return `<button type="button" role="menuitem" aria-haspopup="menu" class="has-sub" data-sub="${n}">${icon}${esc(i.label)}${M_CHEV}</button>`;
    return `<button type="button" role="menuitem" class="${i.cls || ''}" data-act="${i.act}" data-id="${esc(i.id || '')}"${i.k ? ` data-k="${esc(i.k)}"` : ''}${i.url ? ` data-url="${esc(i.url)}"` : ''}>${icon}${esc(i.label)}${i.check ? `<span class="ck">${CHECK}</span>` : ''}</button>`;
  }).join('');
}
function menu(at, items) {
  closeMenu();
  const m = document.createElement('div');
  m.className = 'menu';
  m.setAttribute('role', 'menu');
  const stack = [];
  const show = (list, title) => {
    m.innerHTML = (title ? `<button type="button" class="m-back" data-back="1"><svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m15 6-6 6 6 6"/></svg>${esc(title)}</button><hr>` : '') + menuRows(list);
    m.dataset.depth = String(stack.length);
    place();
    $('button', m).focus({ preventScroll: true });
  };
  m.addEventListener('click', (e) => {
    const sub = e.target.closest('[data-sub]');
    const back = e.target.closest('[data-back]');
    if (!sub && !back) return;
    e.stopPropagation();
    if (sub) { const cur = stack.length ? stack[stack.length - 1].list : items; const it = cur[Number(sub.dataset.sub)]; stack.push({ list: it.sub, title: it.label }); show(it.sub, it.label); }
    else { stack.pop(); const top = stack[stack.length - 1]; show(top ? top.list : items, top ? top.title : ''); }
  });
  document.body.append(m);
  let x, y, right = false;
  if (at instanceof Element) { const r = at.getBoundingClientRect(); x = r.right; y = r.bottom + 6; right = true; m.style.transformOrigin = 'top right'; }
  else { x = at.x; y = at.y; m.style.transformOrigin = 'top left'; }
  function place() {
    const left = right ? x - m.offsetWidth : x;
    m.style.top = Math.max(10, Math.min(innerHeight - m.offsetHeight - 10, y)) + 'px';
    m.style.left = Math.max(10, Math.min(innerWidth - m.offsetWidth - 10, left)) + 'px';
  }
  show(items, '');
}

// The same menu wherever a game is: its "…" button, or a right-click on its tile.
function gameMenu(id) {
  const g = S.library[id], x = byId(id), st = x ? status(x) : g ? 'installed' : 'get';
  const first = st === 'running' ? { act: 'stop', id, label: t('Close game'), icon: 'stop' }
    : g ? { act: 'play', id, label: t('Play'), icon: 'play' }
      : st === 'get' ? (isSaved(id) ? { act: 'install', id, label: t('Install'), icon: 'down' } : { act: 'addlib', id, label: t('Add to Library'), icon: 'plus' }) : null;
  const pinned = pins().includes(id);
  return [
    ...(first ? [first] : []),
    ...(!g && st === 'get' && !isSaved(id) ? [{ act: 'install', id, label: t('Add and Install'), icon: 'down' }] : []),
    ...(g && S.updates[id] ? [{ act: 'install', id, label: t('Update'), icon: 'down' }] : []),
    ...(S.view !== 'game' || S.arg !== id ? [{ act: 'open', id, label: t('Game page'), icon: 'page' }] : []),
    { act: 'pin', id, label: pinned ? t('Unpin from the sidebar') : t('Pin to the sidebar'), icon: 'pin' },
    ...(g ? [{ act: 'fav', id, label: favs().includes(id) ? t('Remove from Favorites') : t('Add to Favorites'), icon: 'star' }] : []),
    ...(!g && isSaved(id) ? [{ act: 'unsave', id, label: t('Remove from Library'), icon: 'bin' }] : []),
    ...(g ? ['-',
      { label: t('Manage'), icon: 'gear', sub: [
        { act: 'auto-up', id, label: t('Update automatically'), icon: 'refresh', check: !!g.auto_update },
        { act: 'versions', id, label: t('Changelog and other versions'), icon: 'history' },
        ...(!g.local ? [{ act: 'build', id, label: t('Change Build…'), icon: 'layers' }] : []),
        { act: 'launch-opts', id, label: t('Launch options…'), icon: 'exe' },
        { act: 'setexe', id, label: t('Choose Program…'), icon: 'exe' },
        ...(S.easy.has(id) ? [{ act: 'setup', id, label: g.game_copy ? t('Change your copy…') : t('Insert your copy…'), icon: 'disc' }] : []),
        ...(looksHere() ? [{ act: 'looks', id, label: t('Looks'), icon: 'look' }] : []),
        ...(S.mods[id] ? [{ act: 'mods', id, label: t('Mods'), icon: 'puzzle' }] : []),
      ] },
      { label: t('Customize'), icon: 'shot', sub: [
        { act: 'rename', id, label: t('Rename…'), icon: 'page' },
        ...(x && x.custom ? [{ act: 'edit-info', id, label: t('Edit Game Info…'), icon: 'page' }] : []),
        { act: 'cover', id, label: g.cover_file ? t('Change Cover…') : t('Set a Cover…'), icon: 'shot' },
        ...(g.cover_file ? [{ act: 'cover-reset', id, label: t('Use the Usual Cover'), icon: 'shot' }] : []),
        { act: 'hero', id, label: g.hero_file ? t('Change Background…') : t('Set a Background…'), icon: 'shot' },
        ...(g.hero_file ? [{ act: 'hero-reset', id, label: t('Use the Usual Background'), icon: 'shot' }] : []),
      ] },
      { label: t('Add To'), icon: 'plus', sub: [
        { act: 'desktop', id, label: t('Add to desktop'), icon: 'desk' },
        ...(S.os === 'windows' || S.os === 'linux' ? [{ act: 'app-menu', id, label: S.os === 'windows' ? t('Add to Start Menu') : t('Add to App Menu'), icon: 'desk' }] : []),
        ...(S.steam ? [{ act: 'steam', id, label: t('Add to Steam'), icon: 'steam' }] : []),
      ] },
      { label: t('Files'), icon: 'folder', sub: [
        { act: 'folder', id, label: t('Open its folder'), icon: 'folder' },
        { act: 'shots', id, label: t('Screenshots'), icon: 'shot' },
        { act: 'backups', id, label: t('Backups of your saves…'), icon: 'save' },
        { act: 'move', id, label: t('Move to another drive…'), icon: 'move' },
      ] },
      ...(x && x.repo ? [{ label: t('On GitHub'), icon: 'web', sub: [{ act: 'url', url: `https://github.com/${x.repo}#readme`, label: t('Read me'), icon: 'page' }, { act: 'url', url: `https://github.com/${x.repo}`, label: t('Its GitHub'), icon: 'web' }] }] : []),
      ...(window.pluginMenu ? window.pluginMenu(id) : []),
      '-',
      { act: 'troubleshoot', id, label: t('Troubleshoot…'), icon: 'info' },
      { act: 'report', id, label: t('Report a Problem…'), icon: 'page' },
    ] : x && x.repo ? ['-', { act: 'url', url: `https://github.com/${x.repo}#readme`, label: t('Read me'), icon: 'page' }, { act: 'url', url: `https://github.com/${x.repo}`, label: t('Its GitHub'), icon: 'web' }] : []),
    ...(g ? ['-', { act: 'uninstall', id, label: t('Uninstall…'), cls: 'danger', icon: 'bin' }] : []),
  ];
}

// The game's folder is its own, right inside one of the libraries (so the launcher may delete it).
// A game brought over from elsewhere, or a local build, keeps its folder.
function ownFolder(g) {
  if (!g || g.local || !g.path) return false;
  const norm = (p) => String(p || '').replace(/[\\/]+$/, '').replace(/\\/g, '/').toLowerCase();
  const parent = norm(g.path).replace(/\/[^/]*$/, '');
  if ([S.settings.library_dir, ...(S.settings.library_dirs || [])].some((l) => l && norm(l) === parent)) return true;
  // A folder the launcher made for it (named after it), in a library folder since removed.
  const made = String(g.id || '').replace(/[^A-Za-z0-9_-]/g, '-').toLowerCase();
  return !!g.asset && norm(g.path).split('/').pop() === made;
}
function confirmUninstall(id) {
  const g = S.library[id];
  const s = document.createElement('div');
  s.className = 'scrim';
  s.innerHTML = `<div class="modal" role="dialog" aria-modal="true" aria-labelledby="u-h"><h2 id="u-h">${esc(tf('Uninstall {name}?', { name: g.name }))}</h2>
    <p>${esc(t('It comes out of your Library. You can install it again from Discover any time.'))}</p>
    ${g.exe && /^([a-z]:[\\/]|\\\\|\/)/i.test(g.exe) && !String(g.exe).toLowerCase().startsWith(String(g.path).toLowerCase()) ? `<p class="muted">${esc(t('Its own installer put the game somewhere else. With the box ticked, its own uninstaller opens too.'))}</p>` : ''}
    ${ownFolder(g) ? `<label><input type="checkbox" id="u-del" checked> ${esc(tf('Also delete its folder ({path}), saves and settings inside it too', { path: g.path }))}</label>` : `<p class="muted">${esc(tf('Its folder stays where it is: {path}', { path: g.path }))}</p>`}
    <div class="acts"><button type="button" class="btn" data-x="no">${esc(t('Keep it'))}</button><button type="button" class="btn danger" data-x="yes">${esc(t('Uninstall'))}</button></div></div>`;
  document.body.append(s);
  $('[data-x="no"]', s).focus();
  s.addEventListener('click', async (e) => {
    const x = e.target.closest('[data-x]');
    if (e.target === s || (x && x.dataset.x === 'no')) return s.remove();
    if (!x) return;
    const del = !!($('#u-del', s) || {}).checked;
    s.remove();
    let r = {};
    try { r = (await invoke('uninstall', { id, deleteFiles: del })) || {}; } catch (err) { await refreshState(); render(true); return fail(err); }
    if (window.usageUninstall) window.usageUninstall(id);
    delete S.updates[id]; await refreshState(); render(true);
    // Some files couldn't go (still in use by something): say where they are.
    if (r.left) toast(tf('{name} is uninstalled, but a few of its files are still in use and stayed in {path}. Close whatever is using them (or restart), then delete that folder.', { name: g.name, path: r.path }), true);
    else if (r.uninstaller) toast(tf('{name} is uninstalled. Its own uninstaller is open to remove the game from where it installed it.', { name: g.name }));
    else toast(tf('{name} is uninstalled.', { name: g.name }));
  });
  s.addEventListener('keydown', (e) => { if (e.key === 'Escape') s.remove(); });
}


// ---------- modals ----------
const uniq = (a) => [...new Set(a.filter(Boolean))];
const fileSrc = (p) => (T.core.convertFileSrc ? T.core.convertFileSrc(p) : p);

function modal(html, onClick, label) {
  const s = document.createElement('div');
  s.className = 'scrim';
  s.innerHTML = `<div class="modal" role="dialog" aria-modal="true" aria-label="${esc(label || '')}">${html}</div>`;
  document.body.append(s);
  const close = () => s.remove();
  // While it's busy (a move under way), clicking outside or Escape doesn't close it.
  s.addEventListener('click', (e) => {
    if (e.target === s || e.target.closest('[data-x="no"]')) return s.dataset.busy ? null : close();
    const x = e.target.closest('[data-x]');
    if (x && onClick) onClick(x, close, e);
  });
  s.addEventListener('keydown', (e) => { if (e.key === 'Escape' && !s.dataset.busy) close(); });
  const f = $('[data-x="yes"], [data-x]', s);
  if (f) f.focus();
  return s;
}

async function loadLibs() {
  try { S.libs = await invoke('libraries'); } catch (e) { S.libs = []; }
  if (S.view === 'settings') render(true);
}

async function addLibrary() {
  const p = await pickFile({ title: t('A folder for games, on any drive'), directory: true }).catch(fail);
  if (!p) return null;
  const path = typeof p === 'string' ? p : p.path;
  await saveSettings({ library_dirs: uniq([...(S.settings.library_dirs || []), path]).filter((d) => d !== S.settings.library_dir) });
  return path;
}

// The first install asks where games go; after that, only when there's more than one library.
async function startInstall(id, dir, tag) {
  if (dir === undefined && !S.library[id] && (!S.settings.asked_location || (S.settings.library_dirs || []).length)) {
    await loadLibs();
    return locationModal(id);
  }
  if (S.running.has(id)) return toast(t('Close the game first.'), true);
  S.jobs[id] = { id, phase: 'prepare', done: 0, total: 0, at: Date.now() };
  render(true);
  try { await invoke('install', { id, dir: dir || null, tag: tag || null }); } catch (err) { delete S.jobs[id]; fail(err); render(true); }
}

// Play, closing another game first if one is running (after asking).
const STARTING = new Set();
// What a Setup Required recomp needs before it plays: its developer's steps, the README, and a way
// to say it's done (then Play is back).
// A release with more than one build for this PC (one per region, say): the player picks which.
function pickBuild(id, names, current) {
  const a = byId(id) || { name: id };
  const go = (file) => { S.jobs[id] = { id, phase: 'prepare', done: 0, total: 0, at: Date.now() }; render(true); invoke('install', { id, dir: null, tag: null, file }).catch((e) => { delete S.jobs[id]; fail(e); render(true); }); };
  if (window.tvPickBuild && window.tvPickBuild(id, names, current, go)) return;
  const btns = names.map((n) => `<button type="button" class="btn${n === current ? ' main' : ''}" data-x="b" data-f="${esc(n)}" title="${esc(n)}">${esc(n)}</button>`);
  if (btns.length % 2) btns[0] = btns[0].replace('class="btn', 'class="span2 btn');
  modal(`<h2>${esc(tf('Which Version of {name}?', { name: a.name }))}</h2><p>${esc(t('This release comes in more than one build, often one for each region. Pick the one that matches your copy of the game. You can switch later from its right-click menu.'))}</p>
    <div class="acts grid2 builds">${btns.join('')}</div>`, (x, close) => { if (x.dataset.x === 'b') { close(); go(x.dataset.f); } }, t('Pick a build'));
}
async function changeBuild(id) {
  try {
    const r = await invoke('release_builds', { id });
    if (!r || (r.builds || []).length < 2) return toast(t('Its release has one build for your PC.'));
    pickBuild(id, r.builds, r.current);
  } catch (e) { fail(e); }
}

// A game whose folder can't be found: on a drive that isn't connected, moved, or deleted. It stays
// in the Library until the player finds it or takes it out.
function missingModal(id) {
  const g = S.library[id];
  if (!g) return;
  const a = libApp(g);
  if (window.tvMissing && window.tvMissing(id)) return;
  modal(`<h2>${esc(tf('Can\'t Find {name}', { name: a.name }))}</h2>
    <p>${esc(t('Its folder isn\'t where it was:'))}</p><p class="mono" dir="ltr">${esc(g.path)}</p>
    <p>${esc(t('If it\'s on a drive that isn\'t connected, connect it and press Try Again. If you moved it, show the launcher where it is now.'))}</p>
    <div class="acts grid2"><button type="button" class="btn" data-x="forget">${esc(t('Remove from Library'))}</button><button type="button" class="btn" data-x="again">${esc(t('Try Again'))}</button><button type="button" class="btn span2 main" data-x="locate">${esc(t('Find Its Folder…'))}</button></div>`, async (x, close) => {
    if (x.dataset.x === 'again') { await refreshState(); close(); render(true); if (S.missing.has(id)) { toast(t('Still not there.'), true); missingModal(id); } return; }
    if (x.dataset.x === 'forget') { close(); try { await invoke('forget_game', { id }); await refreshState(); render(true); } catch (e) { fail(e); } return; }
    if (x.dataset.x === 'locate') {
      const p = await pickFile({ title: tf('Where {name} is now', { name: a.name }), directory: true }).catch(fail);
      if (!p) return;
      try { S.library[id] = await invoke('locate_game', { id, path: typeof p === 'string' ? p : p.path }); close(); await refreshState(); render(true); toast(tf('Found {name}.', { name: a.name })); } catch (e) { fail(e); }
    }
  }, t('Missing'));
}
window.missingModal = missingModal;

// Players who had RecompLauncher before sharing was asked about: asked once, both off unless they
// say yes. Big Picture mode asks with a sheet of its own (tv.js).
function askData() {
  if (S.settings.data_asked || !S.settings.setup_done) return;
  if (document.body.classList.contains('tv-on') && window.tvAskData) return window.tvAskData();
  const pick = { share_usage: false, share_stats: false };
  const swr = (k, b, s) => `<div class="set-row"><div class="k"><b>${esc(b)}</b><span>${esc(s)}</span></div><button type="button" class="switch" role="switch" aria-checked="false" aria-label="${esc(b)}" data-x="sw" data-k="${k}"></button></div>`;
  modal(`<h2>${esc(t('Help Improve RecompLauncher?'))}</h2>
    <p>${esc(t('RecompLauncher can send a little information to help fix problems and see what players use. Both are off unless you turn them on, and you can change this any time in Settings.'))}</p>
    <div class="set-rows">${swr('share_usage', t('Share usage and diagnostics'), t('Your PC\'s parts, which games you play and for how long, and what went wrong when one crashes or won\'t install. No names, no files.'))}
    ${swr('share_stats', t('Share anonymous usage stats'), t('When the launcher is open and which recomps are being played, under a random ID that isn\'t linked to you or your account.'))}</div>
    <div class="acts grid2"><button type="button" class="btn" data-x="what">${esc(t('What\'s sent'))}</button><button type="button" class="btn main" data-x="save">${esc(t('Save'))}</button></div>`, async (x, close) => {
    if (x.dataset.x === 'sw') { const k = x.dataset.k; pick[k] = !pick[k]; x.setAttribute('aria-checked', String(pick[k])); }
    if (x.dataset.x === 'what') invoke('open_url', { url: 'https://recompstore.com/privacy#launcher' }).catch(fail);
    if (x.dataset.x === 'save') { close(); await saveSettings(Object.assign({ data_asked: true }, pick), true); if (S.view === 'settings') render(true); }
  }, t('Help Improve RecompLauncher?'));
}
window.askData = askData;

function setupReqModal(id) {
  // In Big Picture mode it's a sheet made for the controller.
  if (window.tvSetupReq && window.tvSetupReq(id)) return;
  const a = byId(id);
  if (!a) return;
  const inst = S.library[id];
  const steps = (a.setup_steps || []).filter(Boolean);
  const readme = a.setup_readme || (a.repo ? `https://github.com/${a.repo}#readme` : '');
  // Two to a row; with an odd number the first one takes a row of its own, so none sits alone.
  const btns = [readme ? `<button type="button" class="btn" data-x="readme">${esc(t('Open the README'))}</button>` : '',
    inst ? `<button type="button" class="btn" data-x="folder">${esc(t('Open its folder'))}</button>` : '',
    `<button type="button" class="btn" data-x="no">${esc(inst ? t('Not yet') : t('Close'))}</button>`,
    inst ? `<button type="button" class="btn main" data-x="yes">${PLAY}${esc(t('Done, Play'))}</button>` : `<button type="button" class="btn main" data-x="get">${DOWN}${esc(t('Install'))}</button>`].filter(Boolean);
  if (btns.length % 2) btns[0] = btns[0].replace('class="btn', 'class="span2 btn');
  const acts = btns.join('');
  modal(`<h2>${esc(tf('Set up {name}', { name: a.name }))}</h2>
    <p>${esc(t('This one needs a few things on your PC before it plays. Do these once, in order:'))}</p>
    ${steps.length ? `<ol class="setup-steps">${steps.map((x) => `<li dir="auto">${esc(x)}</li>`).join('')}</ol>` : `<p>${esc(t('Its developer\'s README has the steps.'))}</p>`}
    ${inst ? '' : `<p class="muted">${esc(t('Install it first, then come back here.'))}</p>`}
    <div class="acts grid2">${acts}</div>`, async (x, close) => {
    if (x.dataset.x === 'readme') invoke('open_url', { url: readme }).catch(fail);
    if (x.dataset.x === 'folder') invoke('open_folder', { id }).catch(fail);
    if (x.dataset.x === 'get') { close(); startInstall(id); }
    if (x.dataset.x === 'yes') { close(); await saveUi({ setup_ok: Object.assign({}, (S.settings.ui || {}).setup_ok || {}, { [id]: true }) }, true); render(true); playGame(id); }
  }, t('Setup Required'));
}

async function playGame(id, program) {
  if (S.missing && S.missing.has(id)) return missingModal(id);
  if (needsSetup(byId(id)) && !setupDone(id)) return setupReqModal(id);
  // A second press while this one is still on its way (saving a look, the cloud check, ReShade)
  // does nothing.
  if (STARTING.has(id) || S.running.has(id)) return;
  STARTING.add(id);
  try { await playGame2(id, program); } finally { STARTING.delete(id); }
}
window.playGame = playGame;
async function playGame2(id, program) {
  // A look just picked in TV mode is saved before the game starts with it.
  if (window.tvFlushLooks) await window.tvFlushLooks();
  // Cloud saves: asked once per game, and a newer save from another PC put in place first.
  if (window.beforePlay && !(await beforePlay(id))) return;
  const other = [...S.running].find((x) => x !== id);
  const go2 = async (replace) => {
    try { await invoke('play', { id, replace, bigPicture: document.body.classList.contains('tv-on'), program: program || null }); }
    catch (err) {
      const m = String(err && err.message ? err.message : err).match(/^NEED_COPY\|(.*)$/);
      if (m || /^NEED_UPDATE\|/.test(String(err && err.message ? err.message : err))) { delete S.setup[id]; return setupModal(id); }
      if (!blocked(id, err)) fail(err);
      if (/program/i.test(String(err))) { S.nextTab = 'installed'; go('game', id); }
    }
  };
  if (!other) return go2(false);
  const on = S.library[other] || { name: other }, me = S.library[id] || { name: id };
  modal(`<h2>${esc(tf('Close {name}?', { name: on.name }))}</h2><p>${esc(tf('{name} is still running. The launcher closes it and starts {other}. Anything you haven\'t saved in it is lost.', { name: on.name, other: me.name }))}</p>
    <div class="acts"><button type="button" class="btn" data-x="no">${esc(t('Keep playing it'))}</button><button type="button" class="btn main" data-x="yes">${esc(t('Close it and play'))}</button></div>`, (x, close) => { if (x.dataset.x === 'yes') { close(); go2(true); } }, t('Switch games'));
}

// Hold Play: pick which of the game's programs starts this time (the game itself, or its own
// launcher or tools). A normal press starts the usual one.
window.startWith = async function startWith(id) {
  const g = S.library[id];
  if (!g) return;
  let list = [];
  try { list = (await invoke('game_programs', { id })) || []; } catch (e) { /* none */ }
  const usual = String(g.exe || '').replace(/\\/g, '/');
  list = list.filter((p) => !/unins/i.test(p)).slice(0, 12);
  if (list.length < 2) return playGame(id);
  if (window.tvAsk && document.body.classList.contains('tv-on')) {
    const i = await tvAsk(t('Start With'), list.map((p) => ({ label: p.split(/[\\/]/).pop(), sub: p.replace(/\\/g, '/') === usual ? t('Its usual program') : p, icon: 'play' })));
    if (i >= 0) playGame(id, list[i]);
    return;
  }
  const m = modal(`<h2>${esc(t('Start With'))}</h2><p>${esc(tf('Which of {name}\'s programs starts this time. A normal press of Play starts the usual one.', { name: g.name }))}</p>
    <div class="choices">${list.map((p, i) => `<button type="button" class="choice" data-x="pick" data-i="${i}"><span><b>${esc(p.split(/[\\/]/).pop())}</b><small>${esc(p.replace(/\\/g, '/') === usual ? t('Its usual program') : p)}</small></span></button>`).join('')}</div>
    <div class="acts"><button type="button" class="btn" data-x="no">${esc(t('Cancel'))}</button></div>`, (x, close) => {
    if (x.dataset.x !== 'pick') return;
    close();
    playGame(id, list[Number(x.dataset.i)]);
  }, t('Start With'));
  return m;
};
// Pressing and holding a Play button opens Start With instead of playing.
{
  let held = null;
  document.addEventListener('pointerdown', (e) => {
    const b = e.target.closest('[data-act="play"][data-id]');
    if (!b || e.button !== 0 || !S.library[b.dataset.id]) return;
    clearTimeout(held && held.t);
    held = { b, t: setTimeout(() => { b.dataset.held = '1'; startWith(b.dataset.id); }, 600) };
  });
  const stop = () => { if (held) { clearTimeout(held.t); held = null; } };
  document.addEventListener('pointerup', stop);
  document.addEventListener('pointercancel', stop);
  document.addEventListener('pointerleave', stop);
  // The click that ends a hold doesn't also play.
  document.addEventListener('click', (e) => {
    const b = e.target.closest('[data-held]');
    if (b) { delete b.dataset.held; e.stopImmediatePropagation(); e.preventDefault(); }
  }, true);
}

// Windows Security stopped a file: say what happened and what can be done, instead of a raw error.
function blocked(id, err) {
  const m = String(err && err.message ? err.message : err).match(/^BLOCKED\|([^|]*)\|/);
  if (!m) return false;
  const a = byId(id) || { name: (S.library[id] || {}).name || id || t('the update') };
  modal(`<h2>${esc(tf('Windows Security blocked {name}', { name: a.name }))}</h2>
    <p>${htmlFill(t('It stopped {file}, saying it could be harmful. New recomps get flagged by mistake a lot: they aren\'t signed yet, and the way they run old console code looks unusual to it.'), { file: `<b>${esc(m[1])}</b>` })}</p>
    <p>${esc(t('Still, check before you let it through. VirusTotal scans the file with about 70 antivirus programs: if only one or two flag it, it\'s almost certainly a mistake. You decide in Windows Security whether to allow it.'))}</p>
    <div class="acts grid2"><button type="button" class="btn" data-x="vt">${esc(t('Check it on VirusTotal'))}</button><button type="button" class="btn" data-x="ms">${esc(t('Report it to Microsoft'))}</button>
      ${a.repo ? `<button type="button" class="btn" data-x="gh">${esc(t('Tell its developer'))}</button>` : `<button type="button" class="btn" data-x="folder">${esc(t('Open its folder'))}</button>`}<button type="button" class="btn main" data-x="no">${esc(t('Close'))}</button></div>`, (x) => {
    if (x.dataset.x === 'vt') invoke('open_url', { url: 'https://www.virustotal.com/gui/home/upload' }).catch(fail);
    if (x.dataset.x === 'ms') invoke('open_url', { url: 'https://www.microsoft.com/en-us/wdsi/filesubmission' }).catch(fail);
    if (x.dataset.x === 'gh') invoke('open_url', { url: `https://github.com/${a.repo}/issues` }).catch(fail);
    if (x.dataset.x === 'folder') invoke('open_folder', { id }).catch(fail);
  }, t('Blocked by Windows Security'));
  return true;
}
const cleanMsg = (m) => t(String(m || '').replace(/^BLOCKED\|[^|]*\|/, ''));

async function selfUpdate() {
  toast(t('Downloading the new RecompLauncher…'));
  try { await invoke('update_launcher'); } catch (e) { if (!blocked('', e)) fail(e); }
}

function libChoices(libs, sel) {
  return `<div class="choices">${libs.map((l, i) => `<label class="choice"><input type="radio" name="lib" value="${esc(l.path)}"${(sel ? l.path === sel : l.default) ? ' checked' : ''}><span><b>${esc(l.path)}</b><small>${l.free != null ? esc(tf('{free} free', { free: fmtB(l.free) })) : ''}${l.games ? ' · ' + esc(tn(l.games, '1 game', '{n} games')) : ''}${l.default ? ' · ' + esc(t('default')) : ''}</small></span></label>`).join('')}</div>`;
}

function locationModal(id) {
  const a = byId(id) || { name: id };
  const draw = () => `<h2>${esc(tf('Where should {name} go?', { name: a.name }))}</h2>
    <p>${esc(t('Each game gets its own folder in your Recomp Library. Updates always go to the same place, and you can move a game to another drive later.'))}</p>
    ${libChoices(S.libs)}
    <div class="acts" style="justify-content:space-between"><button type="button" class="btn" data-x="add">${esc(t('Add a folder on another drive…'))}</button>
      <span style="display:flex;gap:10px"><button type="button" class="btn" data-x="no">${esc(t('Cancel'))}</button><button type="button" class="btn main" data-x="yes">${esc(t('Install here'))}</button></span></div>`;
  const m = modal(draw(), async (x, close) => {
    if (x.dataset.x === 'add') { const p = await addLibrary(); if (p) { await loadLibs(); $('.modal', m).innerHTML = draw(); const r = $(`input[value="${CSS.escape(p)}"]`, m); if (r) r.checked = true; } return; }
    if (x.dataset.x === 'yes') { const dir = ($('input[name=lib]:checked', m) || {}).value; close(); await saveSettings({ asked_location: true }); startInstall(id, dir || S.settings.library_dir); }
  }, t('Choose where to install'));
}

async function moveModal(id) {
  const g = S.library[id];
  await loadLibs();
  const parent = g.path.replace(/[\\/][^\\/]+$/, '');
  const others = S.libs.filter((l) => l.path !== parent);
  const draw = () => `<h2>${esc(tf('Move {name}', { name: g.name }))}</h2>
    <p>${esc(tf('Its saves, settings and play time go with it. It\'s at {path}.', { path: g.path }))}</p>
    ${others.length ? libChoices(others, others[0].path) : `<p class="muted">${esc(t('You only have one library. Add a folder on another drive to move it there.'))}</p>`}
    <div class="prog" id="mv-prog" hidden><div class="track"><i style="width:0%"></i></div><div class="lbl"><span>${esc(t('Moving…'))}</span><span></span></div></div>
    <div class="acts" style="justify-content:space-between"><button type="button" class="btn" data-x="add">${esc(t('Add a folder…'))}</button>
      <span style="display:flex;gap:10px"><button type="button" class="btn" data-x="no">${esc(t('Cancel'))}</button><button type="button" class="btn main" data-x="yes"${others.length ? '' : ' disabled'}>${esc(t('Move'))}</button></span></div>`;
  const m = modal(draw(), async (x, close) => {
    if (x.dataset.x === 'add') { const p = await addLibrary(); if (p) close(), moveModal(id); return; }
    if (x.dataset.x !== 'yes') return;
    const to = ($('input[name=lib]:checked', m) || {}).value;
    if (!to) return;
    $$('button', m).forEach((b) => (b.disabled = true));
    m.dataset.busy = '1';
    $('#mv-prog', m).hidden = false;
    S.moveWatch = (p) => { if (p.id === id && p.total) { $('#mv-prog i', m).style.width = Math.round((p.done / p.total) * 100) + '%'; $$('#mv-prog .lbl span', m)[1].textContent = tf('{done} of {total}', { done: fmtB(p.done), total: fmtB(p.total) }); } };
    try { await invoke('move_game', { id, to }); close(); toast(tf('{name} is now in {path}.', { name: g.name, path: to })); await refreshState(); render(true); }
    catch (err) { close(); fail(err); }
    S.moveWatch = null;
  }, t('Move a game'));
}

async function backupsModal(id) {
  const g = S.library[id];
  let list = [];
  try { list = (await invoke('backups', { id })) || []; } catch (e) { return fail(e); }
  modal(`<h2>${esc(tf('Backups of {name}', { name: g.name }))}</h2>
    <p>${esc(t('Before every update the launcher keeps a copy of the files that are yours: saves, settings and mods. The newest five are kept.'))}</p>
    ${list.length ? `<div class="choices">${list.map((b) => `<div class="choice"><span><b>${esc(when(b.when))}</b><small>${esc(tn(b.files, '1 file', '{n} files'))} · ${esc(fmtB(b.size) || tf('{n} B', { n: 0 }))}</small></span><button type="button" class="btn" data-x="restore" data-name="${esc(b.name)}">${esc(t('Restore'))}</button></div>`).join('')}</div>` : `<p class="muted">${esc(t('No backups yet.'))}</p>`}
    <div class="acts"><button type="button" class="btn" data-x="no">${esc(t('Close'))}</button><button type="button" class="btn main" data-x="now">${esc(t('Back up now'))}</button></div>`, async (x, close) => {
    if (x.dataset.x === 'now') { try { const b = await invoke('backup_now', { id }); close(); toast(b ? t('Backed up.') : t('Nothing to back up yet: the folder only has the game\'s own files.')); } catch (e) { fail(e); } }
    if (x.dataset.x === 'restore') { try { const n = await invoke('restore_backup', { id, name: x.dataset.name }); close(); toast(tn(n, 'Put back 1 file.', 'Put back {n} files.')); } catch (e) { fail(e); } }
  }, t('Backups'));
}

async function addToSteam(id, restart) {
  try {
    const r = await invoke('add_to_steam', { id, restart });
    if (r === 'steam_running') {
      const v = await (window.ask || ask)(t('Restart Steam?'), `<p>${esc(tf('Steam only picks up new games when it starts, so the launcher closes Steam, adds {name} with its art, and opens Steam again.', { name: S.library[id].name }))}</p>`,
        [{ label: t('Not now'), v: null }, { label: t('Restart Steam'), v: 'yes', main: true }]);
      if (v === 'yes') { toast(t('Adding it to Steam…')); return addToSteam(id, true); }
      return;
    }
    toast(tf('{name} is in your Steam library.', { name: S.library[id].name }));
  } catch (e) { fail(e); }
}

// ---------- screenshots ----------
async function loadAllShots() {
  try { S.allShots = (await invoke('all_screenshots')) || []; } catch (e) { S.allShots = []; }
  if (S.view === 'shots') render(true);
}
const shotList = () => (S.allShots || []).filter((x) => !S.shotsGame || x.id === S.shotsGame);
const when = (ts) => new Date(ts * 1000).toLocaleString(curLocale(), { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' });

// Achievements: a first look at what's coming. Nothing here pretends to work yet; the two buttons
// that do something are Learn More (the site) and Sign In (the account they'll be tied to).
const TROPHY = (n = 20) => `<svg width="${n}" height="${n}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M8 4h8v5a4 4 0 0 1-8 0z"/><path d="M8 5.5H5.2a.2.2 0 0 0-.2.2A3.3 3.3 0 0 0 8 9.3M16 5.5h2.8a.2.2 0 0 1 .2.2 3.3 3.3 0 0 1-3 3.6"/><path d="M12 13v3.5M9 20h6M10 16.5h4l.6 3.5h-5.2z"/></svg>`;
const LOCK = '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="5" y="11" width="14" height="9" rx="2"/><path d="M8 11V8a4 4 0 0 1 8 0v3"/></svg>';
const ACH_SITE = 'https://recompachievements.com';
function achCards() {
  return [
    [t('Unlocks while you play'), t('Achievements pop up the moment you earn them, right in your recomps.'), '<path d="M12 3l2.5 5.2 5.7.8-4.1 4 1 5.6L12 16l-5.1 2.6 1-5.6-4.1-4 5.7-.8z"/>'],
    [t('Your profile and rating'), t('Every achievement adds to your profile and your rating, on every PC you play on.'), '<circle cx="12" cy="8.5" r="3.6"/><path d="M4.8 20a7.2 7.2 0 0 1 14.4 0"/>'],
    [t('Leaderboards'), t('See how you stack up against other players, game by game.'), '<path d="M5 20V12M12 20V5M19 20v-5"/>'],
  ];
}
function pageAchievements() {
  const signed = !!(S.account && S.account.signedIn);
  return `<div class="page ach">
    <div class="head"><div><div class="ach-h"><h1>${esc(t('Achievements'))}</h1><span class="ach-pill">${esc(t('Coming soon'))}</span></div>
      <p class="ach-lead">${esc(t('Earn achievements in your recomps, track them on your profile and climb the leaderboards. We\'re preparing the system. More info soon.'))}</p>${signed ? `<p class="ach-ok">${esc(t('You\'re all set. Your achievements will be tied to your account.'))}</p>` : ''}</div>
      <div class="dctl ach-acts">${signed ? '' : `<button type="button" class="btn" data-act="acc-btn">${esc(t('Sign in'))}</button>`}<button type="button" class="btn main ach-btn" data-act="url" data-url="${ACH_SITE}">${esc(t('Learn more'))}</button></div></div>
    <div class="ach-cards">${achCards().map(([h, d, ic]) => `<div class="ach-card" aria-disabled="true"><span class="ach-lock" aria-hidden="true">${LOCK}</span><span class="ach-ic" aria-hidden="true"><svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">${ic}</svg></span><b>${esc(h)}</b><p>${esc(d)}</p></div>`).join('')}</div>
  </div>`;
}

function pageShots() {
  if (!S.allShots) return `<div class="loading">${esc(t('Loading your screenshots…'))}</div>`;
  const all = S.allShots, list = shotList();
  const games = uniq(all.map((x) => x.id)).map((id) => ({ id, name: (all.find((x) => x.id === id) || {}).game, n: all.filter((x) => x.id === id).length }));
  if (S.shotsGame && !games.some((g) => g.id === S.shotsGame) && S.library[S.shotsGame]) games.push({ id: S.shotsGame, name: S.library[S.shotsGame].name, n: 0 });
  return `<div class="page"><div class="head"><div><h1>${esc(t('Screenshots'))}</h1><p>${esc(t('F12, or View + RB on a controller, while you play. They\'re kept in Pictures\\RecompLauncher.'))}</p></div>
      ${all.length ? `<button type="button" class="btn" data-act="shots-root">${esc(t('Open the folder'))}</button>` : ''}</div>
    ${games.length ? `<div class="chips"><button type="button" class="chip${!S.shotsGame ? ' on' : ''}" data-act="shots-game" data-g="">${esc(t('All'))} ${fmtNum(all.length)}</button>${games.map((g) => `<button type="button" class="chip${S.shotsGame === g.id ? ' on' : ''}" data-act="shots-game" data-g="${esc(g.id)}">${esc(g.name)} ${fmtNum(g.n)}</button>`).join('')}</div>` : ''}
    ${list.length ? `<div class="shotgrid">${list.map((x, i) => `<button type="button" class="shot" data-act="shot-open" data-i="${i}" data-path="${esc(x.path)}"><img src="${esc(fileSrc(x.path))}" alt="" loading="lazy"><span><b>${esc(x.game)}</b><small>${esc(when(x.time))}</small></span></button>`).join('')}</div>`
      : `<div class="empty"><svg width="44" height="44" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${MI.shot}</svg><h2>${esc(t('No screenshots yet'))}</h2><p>${esc(t('While a game is running, press F12 or View + RB. They show up here, ready to share.'))}</p></div>`}
  </div>`;
}

// A big view of one screenshot, with arrows to the others and Share, Show in folder and Delete.
let viewerClose = null;
function viewShot(i, list = shotList()) {
  if (!list.length) return;
  if (viewerClose) viewerClose();
  document.querySelector('.viewer')?.remove();
  const v = document.createElement('div');
  v.className = 'viewer';
  v.setAttribute('role', 'dialog');
  v.setAttribute('aria-label', t('Screenshot'));
  document.body.append(v);
  const draw = () => {
    const x = list[i];
    v.innerHTML = `<div class="v-top"><span><b>${esc(x.game || '')}</b>${x.time ? ` · ${esc(when(x.time))}` : ''}</span><span class="v-count">${esc(tf('{page} of {pages}', { page: i + 1, pages: list.length }))}</span><button type="button" class="icon-btn" data-v="close" aria-label="${esc(t('Close'))}"><svg width="16" height="16" viewBox="0 0 12 12" aria-hidden="true"><path d="m1.5 1.5 9 9m0-9-9 9" stroke="currentColor" stroke-width="1.6"/></svg></button></div>
      <div class="v-img"><button type="button" class="v-nav" data-v="prev" aria-label="${esc(t('Previous'))}"${i ? '' : ' disabled'}><svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m15 5-7 7 7 7"/></svg></button>
        <img src="${esc(fileSrc(x.path))}" alt="${esc(t('Screenshot'))}">
        <button type="button" class="v-nav" data-v="next" aria-label="${esc(t('Next'))}"${i < list.length - 1 ? '' : ' disabled'}><svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 5 7 7-7 7"/></svg></button></div>
      <div class="v-bar"><button type="button" class="btn main" data-v="share"><svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 15V3m0 0L7.5 7.5M12 3l4.5 4.5"/><path d="M5 12v7a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-7"/></svg>${esc(t('Share'))}</button>
        <button type="button" class="btn" data-v="show">${'<svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">' + MI.folder + '</svg>'}${esc(t('Show in folder'))}</button>
        <button type="button" class="btn danger" data-v="delete">${'<svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">' + MI.bin + '</svg>'}${esc(t('Delete'))}</button></div>`;
  };
  const close = () => { v.classList.add('out'); setTimeout(() => v.remove(), 180); document.removeEventListener('keydown', keys, true); if (viewerClose === close) viewerClose = null; };
  viewerClose = close;
  const step = (d) => { const n = i + d; if (n >= 0 && n < list.length) { i = n; draw(); } };
  const keys = (e) => {
    if (e.key === 'Escape') { e.stopPropagation(); close(); }
    else if (e.key === 'ArrowLeft') step(-1);
    else if (e.key === 'ArrowRight') step(1);
  };
  document.addEventListener('keydown', keys, true);
  v.addEventListener('click', async (e) => {
    const b = e.target.closest('[data-v]');
    if (!b) { if (e.target === v || e.target.classList.contains('v-img')) close(); return; }
    const x = list[i];
    switch (b.dataset.v) {
      case 'close': return close();
      case 'prev': return step(-1);
      case 'next': return step(1);
      case 'share': try { await invoke('copy_screenshot', { path: x.path }); toast(t('Copied. Paste it into Discord, X, a message or anywhere.')); } catch (err) { fail(err); } return;
      case 'show': return invoke('show_screenshot', { path: x.path }).catch(fail);
      case 'delete':
        try { await invoke('delete_screenshot', { path: x.path }); } catch (err) { return fail(err); }
        list.splice(i, 1);
        if (S.allShots) S.allShots = S.allShots.filter((s) => s.path !== x.path);
        for (const k of Object.keys(S.shots)) S.shots[k] = (S.shots[k] || []).filter((p) => p !== x.path);
        if (!list.length) { close(); } else { i = Math.min(i, list.length - 1); draw(); }
        render(true);
    }
  });
  window.__closeViewer = close;
  draw();
}

// A newer RecompLauncher: one click downloads it, checks it and runs its installer.
// A new RecompLauncher, announced while the launcher is open: looked for when it opens and when the
// window comes back into view (at most once a minute); the core looks every 5 minutes as well
// ('launcher-update' in more.js). Each new version is announced once.
const LL = { at: 0, told: '' };
async function lookForLauncher(now) {
  if (!now && (S.away || Date.now() - LL.at < 60000)) return;
  LL.at = Date.now();
  let v = null;
  try { v = await invoke('launcher_update'); } catch { return; }
  if (v && v.version) S.launcherUpd = v;
  if (v && v.version && v.version !== LL.told) { LL.told = v.version; updateToast(v); }
  renderSide();
}
window.addEventListener('focus', () => lookForLauncher(false));

// A version too old to keep up (older than the list's "min"): a window, once each time the
// launcher opens, instead of a toast that goes away.
let requiredShown = false;
function updateToast(v) {
  if (v && v.required && !requiredShown) {
    requiredShown = true;
    modal(`<h2>${esc(t('Time to Update'))}</h2><p>${esc(tf('This version of RecompLauncher is too old to keep up with the recomps. Update to {version} to keep playing. It only takes a moment.', { version: verName(v.version) }))}</p>
      <div class="acts"><button type="button" class="btn" data-x="no">${esc(t('Not now'))}</button><button type="button" class="btn main" data-x="yes">${esc(t('Update now'))}</button></div>`, (x, close) => { close(); if (x.dataset.x === 'yes') selfUpdate(); }, t('Time to Update'));
    return;
  }
  toast(tf('RecompLauncher {version} is out.', { version: verName(v.version) }), false, `${esc(tf('RecompLauncher {version} is out.', { version: verName(v.version) }))} <a href="#" data-act="self-update">${esc(t('Update now'))}</a>`, 20000);
}

function resetModal(what) {
  const setup = what === 'setup';
  modal(`<h2>${esc(setup ? t('Run first-time setup again?') : t('Reset settings?'))}</h2>
    <p>${esc(setup ? t('The launcher restarts in first-time setup. Your games, saves and settings stay as they are.') : t('Every setting goes back to how it started. Your games, saves, library folders and pinned games stay.'))}</p>
    <div class="acts"><button type="button" class="btn" data-x="no">${esc(t('Cancel'))}</button><button type="button" class="btn main" data-x="yes">${esc(setup ? t('Run it again') : t('Reset settings'))}</button></div>`, async (x, close) => {
    if (x.dataset.x !== 'yes') return;
    close();
    try {
      S.settings = await invoke('reset_settings', { what });
      if (setup) return location.reload();
      toast(t('Settings are back to how they started.'));
      render(true);
    } catch (e) { fail(e); }
  }, setup ? t('First-time setup') : t('Reset settings'));
}

function aboutModal() {
  modal(`<div class="about"><img src="img/mark.svg" alt="" width="72" height="72"><h2 class="sr">RecompLauncher</h2><img class="about-wm" src="img/wordmark.svg" alt="" height="26">
    <p class="muted">${esc(tf('Version {v}', { v: verName(S.version) }))} · ${OS_NAME[S.os]} ${esc(S.arch)}</p>
    <p>${esc(t('One home for PC recomps: find, install, update and play them, straight from each developer\'s own GitHub.'))}</p>
    <p class="muted" style="font-size:14px">${esc(t('RecompLauncher has no game files and never downloads any. Every recomp runs with your own copy of the game. RecompLauncher is free software under the GPL-3.0; each recomp has its own license, set by its developer.'))}</p>
    ${discordUrl() ? `<div class="acts grid2"><button type="button" class="btn" data-x="site">recomplauncher.com</button><button type="button" class="btn dc-btn" data-x="discord" aria-label="Discord" title="Discord">${DISCORD}</button>
      <button type="button" class="btn" data-x="me">${esc(t('Made by Ripalda'))}</button><button type="button" class="btn" data-x="update">${esc(t('Check for updates'))}</button>
      <button type="button" class="btn span2" data-x="kofi">${HEART}${esc(t('Support Us'))}</button><button type="button" class="btn main span2" data-x="no">${esc(t('Close'))}</button></div>` : `<div class="acts grid2"><button type="button" class="btn" data-x="site">recomplauncher.com</button><button type="button" class="btn" data-x="me">${esc(t('Made by Ripalda'))}</button>
      <button type="button" class="btn span2" data-x="kofi">${HEART}${esc(t('Support Us'))}</button><button type="button" class="btn" data-x="update">${esc(t('Check for updates'))}</button><button type="button" class="btn main" data-x="no">${esc(t('Close'))}</button></div>`}</div>`, async (x) => {
    if (x.dataset.x === 'discord') invoke('open_url', { url: discordUrl() }).catch(fail);
    if (x.dataset.x === 'site') invoke('open_url', { url: 'https://recomplauncher.com' }).catch(fail);
    if (x.dataset.x === 'me') invoke('open_url', { url: 'https://www.elripalda.com' }).catch(fail);
    if (x.dataset.x === 'kofi') invoke('open_url', { url: KOFI }).catch(fail);
    if (x.dataset.x === 'update') checkLauncher(x);
  }, t('About RecompLauncher'));
}

async function loadPads() {
  let p;
  try { p = await invoke('controllers'); } catch (e) { p = []; }
  const same = JSON.stringify(p) === JSON.stringify(S.pads || []);
  S.pads = p;
  // Only Controllers shows them, and only when one comes or goes.
  if (!same && S.view === 'settings' && S.setTab === 'pads' && !document.querySelector('.scrim, .menu')) setSection('pads', false);
}
setInterval(() => { if (S.view === 'settings' && S.setTab === 'pads' && !document.hidden && !S.away) loadPads(); }, 4000);

async function loadShots(id) {
  try { S.shots[id] = await invoke('screenshots', { id }); } catch (e) { S.shots[id] = []; }
  if (S.view === 'game' && S.arg === id) refreshGame();
}

async function scanRoms() {
  if (!(S.settings.rom_dirs || []).length) { S.roms = []; S.owned = {}; render(true); return; }
  toast(t('Looking through your folders…'));
  try { S.roms = await invoke('scan_roms'); await loadOwned(); const added = await addOwned(); toast(tn(S.roms.length, 'Found 1 game file.', 'Found {n} game files.') + ' ' + (added ? tn(added, '1 recomp went into your Library.', '{n} recomps went into your Library.') : tf('{n} have recomps.', { n: Object.keys(S.owned).length }))); } catch (e) { fail(e); }
  render(true);
}
// Recomps for games the scan found go into the Library (not installed yet), ready for Easy Setup.
async function addOwned() {
  const s = saved(), now = Math.floor(Date.now() / 1000);
  const add = Object.keys(S.owned || {}).filter((id) => !S.library[id] && !s[id] && byId(id) && !byId(id).soon && forOS(byId(id)));
  if (add.length) await saveUi({ saved: Object.assign({}, s, Object.fromEntries(add.map((id) => [id, now]))) }, true);
  return add.length;
}
window.addOwned = addOwned;

// Catalog games the launcher sets up itself (copy and settings).
async function loadEasy(quiet) {
  try { S.easy = new Set(await invoke('easy_setup')); } catch (e) { S.easy = new Set(); }
  if (!quiet) render(true);
}
async function loadOwned() {
  try { S.owned = await invoke('rom_matches'); } catch (e) { S.owned = {}; }
  S.owned = ownedHere(S.owned);
}
// Only recomps this PC can play count as found (a Windows-only one on Linux without Proton doesn't).
function ownedHere(o) {
  if (!S.cat || !S.cat.apps.length) return o || {};
  return Object.fromEntries(Object.entries(o || {}).filter(([id]) => { const a = byId(id); return !a || forOS(a); }));
}

// What a program's exit code means, when it's one of the usual Windows ones.
function codeMeaning(code) {
  const c = Number(code);
  if (!Number.isFinite(c) || c === 0) return '';
  const u = c < 0 ? c + 4294967296 : c;
  const M = {
    0xC0000005: N_('it read or wrote memory it shouldn\'t (access violation)'),
    0xC0000409: N_('it stopped itself after a serious error'),
    0xC0000374: N_('its memory got corrupted (heap corruption)'),
    0xC00000FD: N_('it ran out of stack space'),
    0xC000001D: N_('your processor doesn\'t have an instruction it needs'),
    0xC0000135: N_('a DLL it needs is missing'),
    0xC0000139: N_('a DLL it needs is too old'),
    0xC000007B: N_('a DLL it needs is the wrong kind (32-bit vs 64-bit)'),
    0xC0000142: N_('a DLL it needs failed to start'),
    0xC0000022: N_('Windows denied it access'),
    0xC0E90002: N_('Windows blocked it (Smart App Control)'),
    0xE0434352: N_('a .NET error it didn\'t handle'),
    0x80000003: N_('it hit a debug breakpoint'),
    0x8007000E: N_('it ran out of memory'),
    0xC0000017: N_('it ran out of memory'),
    0x80131506: N_('.NET itself failed'),
    0x80008083: N_('the .NET version it needs isn\'t installed'),
    0x80008096: N_('the .NET version it needs isn\'t installed'),
    0xCFFFFFFF: N_('it stopped responding and Windows closed it'),
    0x40000015: N_('it shut itself down after a fatal error'),
  };
  return M[u] ? t(M[u]) : '';
}
async function crashModal(p) {
  const g = S.library[p.id] || { name: p.id };
  // A community server that stopped at once: on its first start it writes its settings file and
  // waits for them to be filled in. Not a crash.
  if (isServer(byId(p.id) || { id: p.id }) && p.secs < 10) {
    return toast(t('The server made its settings file (server.toml) in its folder. Fill it in, then start it again.'), false, `${esc(t('The server made its settings file (server.toml) in its folder. Fill it in, then start it again.'))} <a href="#" data-act="folder" data-id="${esc(p.id)}">${esc(t('Open its folder'))}</a>`, 15000);
  }
  const a = byId(p.id);
  let log = null;
  try { log = await invoke('recent_log', { id: p.id, since: p.started || 0 }); } catch (e) { /* none */ }
  // It had already shut down cleanly and fell over on the way out: a bug in the game, not
  // something the player can fix, so no crash window.
  const tail = String((log && log[1]) || '').split('\n').slice(-8).join('\n');
  if (p.secs > 20 && /destruct ResourceManager|shutting down|shutdown complete|clean exit|exiting\.\.\./i.test(tail)) return;
  const meaning = codeMeaning(p.code);
  const codeShown = meaning ? `${p.code}: ${meaning}` : String(p.code);
  const details = `${g.name} ${g.tag || ''} closed with code ${codeShown} after ${p.secs} s on ${OS_NAME[S.os]} (RecompLauncher ${S.version}).` + (log ? `\n\n${log[0]}:\n${log[1]}` : '');
  // The game said it couldn't open one of its own files (the player's copy isn't where it looks).
  const EXT = '(?:xex|rpx|rpl|elf|dol|iso|wbfs|rvz|gcm|xci|nsp|nca|z64|n64|v64|bin|cue|chd|cso|pbp|xbe|self|sprx|szs|arc|pak|rarc|cpk|afs)';
  // ReXGlue recomps ask for the game's files with --game_data_root: the player picks the folder once.
  const needRoot = /--game_data_root was not provided/i.test(String((log && log[1]) || '') + ' ' + String(p.said || ''));
  // Or the folder it was told about isn't there anymore (moved, renamed or deleted).
  const gone = String((log && log[1]) || '').match(/game_data_root does not exist:\s*([^\r\n]+)/i);
  const said = String((log && log[1]) || '') + '\n' + String(p.said || '');
  // A folder it asks for on its command line ("--game <folder with code/ content/ meta/>"), or the
  // one already given there that doesn't have the file it looked for (default.xex one folder
  // down): the player picks it, and the launcher finds the right folder inside.
  let rootFlag = needRoot || gone ? '--game_data_root' : '', need = needRoot || gone ? ['default.xex'] : [];
  const ask = !rootFlag && said.match(/(--[a-z][\w-]*)[ =]<([^>]*\b(?:folder|dir|directory|path)\b[^>]*)>/i);
  if (ask) { rootFlag = ask[1]; need = (ask[2].match(/[\w.-]+(?=\/)/g) || []); }
  const gotFlag = !rootFlag && String(g.args || '').match(/(--[\w-]*(?:root|dir|path|game|data)[\w-]*)[ =]("[^"]*"|\S+)/i);
  // The graphics card or its driver can't run it.
  const gpu = /no Vulkan [\d.]+ device|doesn't implement the VK_KHR_surface|unable to create a d3d12 device|volkInitialize failed|vkCreateInstance failed|VK_ERROR_INCOMPATIBLE_DRIVER|vulkan (?:is )?not (?:supported|available)|does not support vulkan|failed to (?:create|initiali[sz]e) (?:the )?(?:vulkan|d3d12|directx 12)/i.test(said);
  // OpenGL in particular: often a game that can draw another way, in its Game settings.
  const gl = /failed to initiali[sz]e opengl|opengl loader|(?:couldn't|could not|unable to|failed to) create (?:an? )?(?:opengl|gl) context|wglCreateContext failed/i.test(said);
  let su = S.setup[p.id] && S.setup[p.id] !== 'loading' ? S.setup[p.id] : null;
  if (gl && !su) { try { su = await invoke('game_setup', { id: p.id }); } catch (e) { /* none */ } }
  const glSetting = gl && su && (su.settings || []).find((o) => /renderer|graphics api|output|backend/i.test(o.label) && o.choices.length > 1);
  // The graphics driver stopped answering while it drew (a driver reset): DXGI device removed/hung.
  const gpuReset = !gpu && /887A0005|887A0006|DXGI_ERROR_DEVICE_(?:REMOVED|HUNG|RESET)|device (?:was )?removed|VK_ERROR_DEVICE_LOST/i.test(said);
  // .NET it needs isn't on this PC (the .NET host says which, with a link).
  const net = /To install missing framework, download:\s*(https:\/\/\S+)/i.exec(said);
  const dotnet = !!net || [-2147450730, -2147450749].includes(Number(p.code)) || /You must install or update \.NET|Failed to resolve hostfxr|hostfxr\.dll \[not found\]/i.test(said);
  const dotnetUrl = net && /^https:\/\/(?:aka\.ms|dotnet\.microsoft\.com)\//i.test(net[1]) ? net[1] : 'https://dotnet.microsoft.com/download';
  // It can't write to its own folder (installed somewhere protected, like Program Files).
  const noWrite = /can't write to its folder|cannot write to (?:its|the game) folder|access (?:is|was) denied|permission denied|read-only file system/i.test(said);
  // A PS3 port that needs the update's decrypted EBOOT (an encrypted one renamed doesn't count).
  const eboot = /No decrypted EBOOT\.elf|isn't a decrypted EBOOT|EBOOT\.elf (?:file )?(?:is not|isn't) valid|E1\s+FAIL/i.test(said);
  // An Xbox 360 recomp built for a title update the player's copy doesn't have.
  const needTU = /title update it was generated with|needs the title update|title update (?:is )?(?:missing|not found)/i.test(said);
  // It wanted the player to pick a ROM in its own window and none was picked.
  const noRom = /No ROM selected/i.test(said);
  // A look (ReShade) was on and it closed soon after starting: offer to play without it.
  const lookOn = !!(g.look && p.secs < 60);
  // Windows couldn't load a file it needs (a DLL that's missing, or too old for it).
  const dll = [-1073741511, -1073741515, 3221225785, 3221225781].includes(Number(p.code));
  // It didn't understand the player's Launch options.
  const badOpts = !!String(g.args || '').trim() && /unknown (?:option|argument|flag|parameter)|unrecognized (?:option|argument|command.line)|invalid (?:option|argument)|unexpected argument/i.test(said);
  // It printed how it wants to be started (it needs the player's files passed to it).
  const usage = !rootFlag && !badOpts && /^\s*usage:/im.test(said);
  // The player's copy isn't the version it's made for.
  const vram = /OUT_OF_DEVICE_MEMORY|vmaCreate\w+\(.*failed: -2\b|out of (?:video|device|gpu) memory/i.test(said);
  const wrongDisc = /no game archives|not the right disc|wrong disc/i.test(said);
  // Windows itself stopped the program: Smart App Control, or an app control policy.
  const sac = [-1058471934, 3236495362].includes(Number(p.code)) || /blocked by (?:smart app control|your organization|app control)/i.test(said);
  // The release has a program for each version of the game, and this one isn't for the player's.
  const otherBuild = /use the build that matches|recompiled from a different version of the game executable/i.test(said);
  const wrongCopy = /wrong rom|rom is not supported|rom (?:file )?is truncated|truncated or invalid|invalid rom|cartridge id at|bad n64 magic|validation failed|isn't the (?:pal|ntsc|usa|us|european|japanese|japan|europe)\b|failed validation|re-dump|verify your file matches|hash mismatch|doesn't match any known|not a supported (?:dump|rom|version)|unsupported (?:rom|dump|version|revision)|wrong (?:region|revision)|bad dump|country byte|rejected: .{0,40}expected|region mismatch|sha1 [0-9a-f]{6,}.{0,30}another release|another release of (?:the|this) game/i.test(said);
  // It says to run its own setup first: offer the setup program that came with it.
  let setupExe = '';
  if (/run (?:the |its )?setup|setup to unpack|setup first/i.test(said)) {
    try { setupExe = ((await invoke('game_programs', { id: p.id })) || []).find((x) => { const n = x.split(/[\\/]/).pop(); return /setup|install|unpack|extract/i.test(n) && !/unins/i.test(n); }) || ''; } catch (e) { /* none */ }
  }
  const miss = log && (String(log[1]).match(new RegExp(`(?:cannot open|can't open|couldn't open|failed to open|not found|no such file|missing)[^\\n]*?([^\\s'"\\\\/:]+\\.${EXT})\\b`, 'i')) || String(log[1]).match(new RegExp(`([^\\s'"\\\\/:]+\\.${EXT})\\b[^\\n]*?(?:not found|missing|cannot be opened)`, 'i')));
  if (miss && gotFlag) { rootFlag = gotFlag[1]; need = [miss[1]]; }
  // A fix it can offer: that's the main button, and sending the report comes second.
  const fix = sac || otherBuild || badOpts || rootFlag || dotnet || noWrite || lookOn || glSetting || dll || setupExe || usage;
  const m = modal(`<h2>${esc(tf('{name} closed unexpectedly', { name: g.name }))}</h2>
    <p>${sac ? esc(t('Windows blocked it. Smart App Control (or an app policy on this PC) stops programs that aren\'t signed yet, and this recomp isn\'t. You can turn Smart App Control off in Windows Security, under App & browser control. Windows may not let you turn it back on later, so it\'s your call; the other choice is to wait for the developer to sign their releases.')) : badOpts ? esc(tf('It didn\'t understand its Launch options ({options}). Clear them and play again.', { options: String(g.args).trim() })) : rootFlag && gone ? esc(tf('It\'s looking for your game files in {folder}, which isn\'t there anymore. Pick the folder with your copy of the game and it starts from there next time.', { folder: gone[1].trim() })) : rootFlag && miss ? esc(tf('It couldn\'t find {file} in the folder it was given. Pick the folder with your copy of the game and the launcher finds it inside.', { file: miss[1] })) : rootFlag ? esc(t('It needs to know where your game files are. Pick the folder with your copy of the game and it starts from there next time.')) : eboot ? esc(t('It needs the decrypted EBOOT.elf from the game\'s update, in the update\'s USRDIR folder. A decrypted one starts with the letters ELF; the normal EBOOT.BIN renamed won\'t work. It\'s made from your own update, for example with RPCS3.')) : needTU ? esc(t('It needs the game\'s title update (the last patch for the game) along with your disc. Its developer\'s instructions say which one and where it goes.')) : dotnet ? esc(t('It needs Microsoft .NET, and the version it needs isn\'t on this PC. Install it, then play again.')) : noWrite ? esc(t('It can\'t write to its own folder, which is somewhere Windows protects (like Program Files). Move it to your Recomp Library in its Manage tab, then play again.')) : noRom ? esc(t('It asks for your ROM in its own window the first time it starts, and none was picked. Play again and choose your ROM when it asks.')) : gpuReset ? esc(t('Your graphics driver stopped responding while the game was drawing. Update your graphics driver from NVIDIA, AMD or Intel; if it keeps happening, try another graphics option in its Game settings.')) : lookOn && p.secs < 30 ? esc(t('It closed soon after starting with a Look on. Some games don\'t get along with ReShade: try it without the Look.')) : gl ? esc(glSetting ? tf('Your graphics driver couldn\'t start OpenGL for it. Switch {setting} in its Game settings, or update your graphics driver, then play again.', { setting: glSetting.label }) : t('Your graphics driver couldn\'t start OpenGL for it. Update your graphics driver from NVIDIA, AMD or Intel, then play again.')) : vram ? esc(t('Your graphics card ran out of memory for it. Close other games and apps using it, lower its resolution in Game settings, then play again.')) : gpu ? esc(t('Your graphics card or its driver can\'t run it: it needs DirectX 12 or Vulkan. Update your graphics driver from NVIDIA, AMD or Intel, then play again.')) : dll ? esc(t('Windows couldn\'t load a file it needs: a DLL that\'s missing or too old. Install the latest Microsoft Visual C++ Redistributable (x64), keep Windows up to date, then play again.')) : setupExe ? esc(t('It needs its setup run first, to unpack the game from your copy.')) : usage ? esc(t('It needs to be started with your game files. Its developer\'s instructions say what to add; put that in its Launch options.')) : wrongDisc ? esc(t('The disc image it was given isn\'t this game\'s, or it\'s missing the game\'s files. Pick your own disc image of the game this recomp is made for.')) : otherBuild ? esc(t('This recomp comes with a program for each version of the game, and the one it started isn\'t for your copy. Choose the program that matches your copy and play again.')) : wrongCopy ? esc(t('Your copy of the game isn\'t the version this recomp is made for. Its developer\'s instructions say which one to use (region and revision).')) : p.said ? esc(p.said) : miss ? esc(tf('It couldn\'t find {file}, part of your copy of the game. Put your game files where its developer\'s instructions say, then play again.', { file: miss[1] })) : esc(p.secs < 5 ? tf('It stopped with code {code} right after it started. That usually means it couldn\'t find your game files or something it needs on this PC.', { code: codeShown }) : tf('It stopped with code {code}.', { code: codeShown }))}</p>
    ${log ? `<div class="sec-h" style="margin:4px 0 6px"><span class="muted" style="font-size:13px">${esc(tf('From {file}', { file: log[0] }))}</span><button type="button" class="linkbtn" data-x="copy">${esc(t('Copy details'))}</button></div><pre class="logbox">${esc(log[1])}</pre>` : ''}
    <textarea class="field" id="c-note" rows="2" placeholder="${esc(t('What were you doing when it closed? A sentence helps the developer find it.'))}" aria-label="${esc(t('What happened'))}"></textarea>
    <div class="acts ${fix ? 'grid2' : 'grid3'}"><button type="button" class="btn" data-x="folder">${esc(t('Open its folder'))}</button><button type="button" class="btn" data-x="trouble">${esc(t('Troubleshoot'))}</button><button type="button" class="btn${fix ? '' : ' main'}" data-x="send">${esc(t('Send it to Ripalda'))}</button>
      ${sac ? `<button type="button" class="btn main" data-x="sac">${esc(t('Open Windows Security'))}</button>` : otherBuild ? `<button type="button" class="btn main" data-x="pickexe">${esc(t('Choose Program'))}</button>` : badOpts ? `<button type="button" class="btn main" data-x="clearopts">${esc(t('Clear launch options'))}</button>` : rootFlag ? `<button type="button" class="btn main" data-x="gdr">${esc(t('Pick your game files'))}</button>` : dotnet ? `<button type="button" class="btn main" data-x="dotnet">${esc(t('Get .NET'))}</button>` : noWrite ? `<button type="button" class="btn main" data-x="manage">${esc(t('Manage'))}</button>` : lookOn ? `<button type="button" class="btn main" data-x="nolook">${esc(t('Play Without the Look'))}</button>` : glSetting ? `<button type="button" class="btn main" data-x="gset">${esc(t('Game settings'))}</button>` : dll ? `<button type="button" class="btn main" data-x="vcredist">${esc(t('Get Visual C++'))}</button>` : setupExe ? `<button type="button" class="btn main" data-x="setup">${esc(t('Run its setup'))}</button>` : usage ? `<button type="button" class="btn main" data-x="opts">${esc(t('Launch options'))}</button>` : a ? `<button type="button" class="btn" data-x="report">${esc(t('Report on GitHub'))}</button>` : `<button type="button" class="btn" data-x="copy">${esc(t('Copy details'))}</button>`}<button type="button" class="btn" data-x="no">${esc(t('Close'))}</button></div>`, async (x, close) => {
    if (x.dataset.x === 'copy') { try { await navigator.clipboard.writeText(details); toast(t('Copied.')); } catch (e) { toast(t('Couldn\'t copy.'), true); } }
    if (x.dataset.x === 'folder') invoke('open_folder', { id: p.id }).catch(fail);
    if (x.dataset.x === 'trouble') { close(); return troubleshootModal(p.id, codeShown); }
    if (x.dataset.x === 'sac') invoke('open_app_control').catch(fail);
    if (x.dataset.x === 'pickexe') { close(); const el = document.createElement('i'); el.dataset.id = p.id; el.dataset.act = 'setexe'; return act(el, new Event('click')); }
    if (x.dataset.x === 'setup') { try { await invoke('run_program', { id: p.id, rel: setupExe }); close(); toast(t('Its setup is open. Play again when it\'s done.')); } catch (e) { fail(e); } }
    if (x.dataset.x === 'opts') { close(); launchOptions(p.id); }
    if (x.dataset.x === 'gset') { close(); S.nextTab = 'settings'; go('game', p.id); }
    if (x.dataset.x === 'dotnet') invoke('open_url', { url: dotnetUrl }).catch(fail);
    if (x.dataset.x === 'manage') { close(); S.nextTab = 'installed'; go('game', p.id); }
    if (x.dataset.x === 'nolook') { close(); if (!S.looks[p.id] || S.looks[p.id] === 'loading') S.looks[p.id] = { look: g.look, amount: g.look_amount }; await setLook(p.id, ''); playGame(p.id); }
    if (x.dataset.x === 'vcredist') invoke('open_url', { url: 'https://learn.microsoft.com/cpp/windows/latest-supported-vc-redist' }).catch(fail);
    if (x.dataset.x === 'clearopts') { try { S.library[p.id] = await invoke('set_game_options', { id: p.id, args: '', autoUpdate: null }); close(); toast(t('Cleared. Press Play to start it.')); } catch (e) { fail(e); } }
    if (x.dataset.x === 'gdr') {
      const dir = await pickFile({ title: t('Pick your game files'), directory: true }).catch(fail);
      const path = dir && (typeof dir === 'string' ? dir : dir.path);
      if (!path) return;
      let root = path;
      try { root = await invoke('game_root', { dir: path, need }); } catch (e) { /* use what was picked */ }
      if (!root) return toast(tf('{file} isn\'t in that folder or the ones inside it. Pick the folder with your copy of the game.', { file: need.join(', ') }), true);
      const cur = String((S.library[p.id] || {}).args || '').replace(new RegExp(`${rootFlag}[ =]("[^"]*"|\\S+)`, 'g'), '').trim();
      try { S.library[p.id] = await invoke('set_game_options', { id: p.id, args: `${cur} ${rootFlag} "${root}"`.trim(), autoUpdate: null }); close(); toast(t('Saved. Press Play to start it.')); } catch (e) { fail(e); }
    }
    if (x.dataset.x === 'report' && a) invoke('open_url', { url: `https://github.com/${a.repo}/issues` }).catch(fail);
    if (x.dataset.x === 'send') {
      const box = $('#c-note', m);
      const note = ((box || {}).value || '').trim();
      if (note.replace(/\s/g, '').length < 12) { toast(t('Write a sentence about what happened first. It\'s how the developer finds it.'), true); if (box) { box.classList.add('need'); box.focus(); } return; }
      x.disabled = true; x.textContent = t('Sending…');
      try { const r = await invoke('send_report', { id: p.id, message: `Closed with code ${codeShown} after ${p.secs} s.\n\n${note}`, log: log ? log[0] : null, kind: null }); close(); toast(r ? t(r) : t('Sent. Thanks for helping.')); }
      catch (e) { x.disabled = false; x.textContent = t('Send it to Ripalda'); fail(e); }
    }
  }, t('The game closed'));
}

// The player's own name for a game: shown everywhere in the launcher instead of its own.
// Add a recomp by its GitHub link: one the catalog lists opens its page; another is read first
// (its game, console and art), shown to the player to check, then added to the Library.
function githubModal() {
  const m = modal(`<h2>${esc(t('Add From GitHub'))}</h2><p>${esc(t('Paste the link to a recomp\'s GitHub page. The launcher reads its releases and adds it to your Library, ready to install and keep up to date.'))}</p>
    <input class="field" id="ghl" type="url" placeholder="https://github.com/owner/name" spellcheck="false" autocomplete="off" aria-label="${esc(t('GitHub link'))}">
    <p class="err" id="ghe" hidden></p>
    <div class="acts grid2"><button type="button" class="btn" data-x="no">${esc(t('Cancel'))}</button><button type="button" class="btn main" data-x="yes">${esc(t('Next'))}</button></div>`, async (x, close) => {
    if (x.dataset.x !== 'yes') return;
    const link = $('#ghl', m).value.trim(), err = $('#ghe', m);
    if (!link) return $('#ghl', m).focus();
    x.disabled = true; err.hidden = true;
    const was = x.textContent; x.textContent = t('Reading it…');
    try {
      const r = await invoke('github_lookup', { link, sgdbKey: (S.settings.ui || {}).sgdb_key || null });
      close();
      if (r.known) { toast(tf('{name} is already in Discover. Here it is.', { name: r.known.name })); return go('game', r.known.id); }
      gameInfoModal({ link, info: r });
    } catch (e) {
      err.textContent = String(e && e.message ? e.message : e); err.hidden = false;
      x.disabled = false; x.textContent = was;
    }
  }, t('Add From GitHub'));
  const f = $('#ghl', m); f.focus();
  f.addEventListener('keydown', (e) => { if (e.key === 'Enter') $('[data-x="yes"]', m).click(); });
}

// Consoles to pick from: the catalog's, then the usual ones.
const MORE_CONSOLES = [['n64', 'Nintendo 64'], ['x360', 'Xbox 360'], ['xbox', 'Xbox'], ['ps1', 'PlayStation'], ['ps2', 'PlayStation 2'], ['ps3', 'PlayStation 3'], ['psp', 'PSP'], ['gc', 'GameCube'], ['wii', 'Wii'], ['wiiu', 'Wii U'], ['switch', 'Nintendo Switch'], ['3ds', 'Nintendo 3DS'], ['nds', 'Nintendo DS'], ['gba', 'Game Boy Advance'], ['gb', 'Game Boy'], ['snes', 'Super Nintendo'], ['nes', 'NES'], ['genesis', 'Sega Genesis'], ['saturn', 'Sega Saturn'], ['dc', 'Dreamcast'], ['pc', 'PC']];
function consoleOptions(cur) {
  const seen = new Set(), out = [];
  for (const o of (S.cat && S.cat.origins) || []) if (!seen.has(o.name.toLowerCase())) { seen.add(o.name.toLowerCase()); seen.add(o.id); out.push([o.id, o.name]); }
  for (const [id, n] of MORE_CONSOLES) if (!seen.has(id) && !seen.has(n.toLowerCase())) { seen.add(id); out.push([id, n]); }
  out.sort((a, b) => a[1].localeCompare(b[1]));
  return `<option value="">${esc(t('Not sure'))}</option>` + out.map(([id, n]) => `<option value="${esc(id)}"${id === cur ? ' selected' : ''}>${esc(n)}</option>`).join('');
}

// The game's details to check before it's added (`link`) or to change after (`id`): name, which
// game it is, its console and its art.
function gameInfoModal({ link, info, id }) {
  const key = (S.settings.ui || {}).sgdb_key || '';
  const cur = { name: info.name || '', game: info.game || '', origin: info.origin || '', cover: info.cover || '', hero: info.hero || '', logo: info.logo || '' };
  let art = { covers: [], heroes: [], logos: [] };
  const take = (a, fresh) => {
    art = { covers: (a && a.covers) || [], heroes: (a && a.heroes) || [], logos: (a && a.logos) || [] };
    if (art.covers.length && (!cur.cover || link || fresh)) cur.cover = artUrl(art.covers[0]);
    if (art.heroes.length && (!cur.hero || link || fresh)) cur.hero = artUrl(art.heroes[0]);
    if (art.logos.length && (!cur.logo || link || fresh)) cur.logo = artUrl(art.logos[0]);
  };
  take(info.art);
  if (link && !cur.hero) cur.hero = info.hero_default || '';
  const sim = info.similar ? `<div class="gi-sim"><span>${esc(tf('This looks like {name}, which is already in Discover.', { name: info.similar.name }))}</span><button type="button" class="btn sm" data-x="similar">${esc(t('See It'))}</button></div>` : '';
  const m = modal(`<h2>${esc(link ? t('Check the Details') : t('Edit Game Info'))}</h2>${sim}
    <div class="gi"><div class="gi-cover" id="gi-cover"></div>
      <div class="gi-fields">
        <label><span>${esc(t('Name'))}</span><input class="field" id="gi-name" value="${esc(cur.name)}" maxlength="120" spellcheck="false"></label>
        <label><span>${esc(t('Game'))}</span><input class="field" id="gi-game" value="${esc(cur.game)}" maxlength="120" spellcheck="false" placeholder="${esc(t('The game it\'s a recomp of'))}"></label>
        <label><span>${esc(t('Console'))}</span><select class="field" id="gi-origin">${consoleOptions(cur.origin)}</select></label>
      </div></div>
    <div class="gi-hero" id="gi-hero" aria-label="${esc(t('Background'))}"></div>
    ${key ? `<div class="acts grid2 gi-art"><button type="button" class="btn sm" data-x="find">${esc(t('Find Art'))}</button><button type="button" class="btn sm" data-x="choose">${esc(t('Choose Art…'))}</button></div>`
      : `<p class="muted gi-hint">${esc(t('Add your own SteamGridDB key in Settings → Games and folders to get covers and backgrounds.'))}</p>`}
    <p class="err" id="gi-err" hidden>${esc(info.art_error || '')}</p>
    <div class="acts grid2"><button type="button" class="btn" data-x="no">${esc(t('Cancel'))}</button><button type="button" class="btn main" data-x="yes">${esc(link ? t('Add to Library') : t('Save'))}</button></div>`, async (x, close) => {
    const err = $('#gi-err', m);
    const read = () => { cur.name = $('#gi-name', m).value.trim(); cur.game = $('#gi-game', m).value.trim(); cur.origin = $('#gi-origin', m).value; };
    switch (x.dataset.x) {
      case 'similar': close(); return go('game', info.similar.id);
      case 'choose': return artPicker(art, cur, () => paint());
      case 'find': {
        read();
        x.disabled = true; err.hidden = true;
        try { take(await invoke('github_art', { game: cur.game || cur.name, sgdbKey: key }), true); paint(); if (!art.covers.length) { err.textContent = t('SteamGridDB has no art under that name. Try the game\'s full name.'); err.hidden = false; } } catch (e) { err.textContent = String(e && e.message ? e.message : e); err.hidden = false; }
        x.disabled = false;
        return;
      }
      case 'yes': {
        read();
        if (!cur.name) return $('#gi-name', m).focus();
        x.disabled = true;
        try {
          if (link) {
            const r = await invoke('add_github', { link, info: cur });
            close();
            await loadCatalog(false);
            if (!r.known && !S.library[r.id]) await saveUi({ saved: Object.assign({}, saved(), { [r.id]: Math.floor(Date.now() / 1000) }) }, true);
            toast(tf('{name} is in your Library. Install it whenever you like.', { name: r.name }));
            go('game', r.id);
          } else {
            await invoke('update_mine', { id, info: cur });
            close();
            await loadCatalog(false);
            render(true);
            toast(t('Saved.'));
          }
        } catch (e) { err.textContent = String(e && e.message ? e.message : e); err.hidden = false; x.disabled = false; }
      }
    }
  }, link ? t('Check the Details') : t('Edit Game Info'));
  function paint() {
    const box = $('#gi-cover', m);
    box.innerHTML = cur.cover ? pic(cur.cover) : `<span>${esc(cur.name.slice(0, 2).toUpperCase())}</span>`;
    const hero = $('#gi-hero', m);
    hero.innerHTML = cur.hero ? pic(cur.hero) : '';
    hero.hidden = !cur.hero;
    for (const b of $$('[data-x="choose"]', m)) b.disabled = !art.covers.length && !art.heroes.length;
  }
  paint();
  const e = $('#gi-err', m); e.hidden = !info.art_error;
  $('#gi-name', m).focus();
}
// Troubleshoot (Beta): what could keep a game from starting, each with a fix where there is one.
const TS_FIX = {
  locate: [N_('Find Its Folder'), (id) => missingModal(id)],
  setexe: [N_('Choose Program…'), (id) => actOn(id, 'setexe')],
  reinstall: [N_('Reinstall'), (id) => actOn(id, 'versions')],
  proton: [N_('Open Settings'), () => { S.setTab = 'pc'; go('settings'); }],
  unblock: [N_('Unblock'), async (id) => { const n = await invoke('unblock_game', { id }); toast(tn(n, 'Unblocked 1 file.', 'Unblocked {n} files.')); }],
  vcredist: [N_('Get It'), () => invoke('open_url', { url: 'https://aka.ms/vs/17/release/vc_redist.x64.exe' })],
  copy: [N_('Pick Your Copy'), (id) => setupModal(id)],
  move: [N_('Move…'), (id) => moveModal(id)],
  clean: [N_('Try a Clean Start'), async (id) => { await invoke('clean_start_next', { id }); playGame(id); }],
};
// What Troubleshoot can say (from troubleshoot in lib.rs), so they get translated.
const TS_TEXT = [
  N_('How it closed last time'),
  N_('Code {code}'),
  N_('A Look (ReShade) and your own launch options are on. If it won\'t start, try once without them.'),
  N_('A Look (ReShade) is on. If it won\'t start, try once without it.'),
  N_('Almost every Windows recomp needs it.'),
  N_('Antivirus may have taken it out, or the download was cut short.'),
  N_('Close it (check the taskbar and the tray), then press Play.'),
  N_('Extras are on'),
  N_('If it won\'t start, a native Linux build (when there is one) usually runs better.'),
  N_('Install Steam with Proton, or umu-launcher, or Wine, then press Play again.'),
  N_('It looks like a Linux build. Reinstall the game to get its Windows build.'),
  N_('It starts {name}'),
  N_('It\'s a Windows version and Proton isn\'t installed'),
  N_('It\'s a Windows version, run through Proton'),
  N_('Its Linux start script is used'),
  N_('Its first-time setup hasn\'t finished'),
  N_('Its folder can\'t be written to'),
  N_('Its folder is there'),
  N_('Its folder isn\'t there'),
  N_('Many recomps draw with Vulkan. Updating the graphics driver adds it.'),
  N_('Press Play and go through the setup window to the end.'),
  N_('That\'s not a Windows program'),
  N_('The Visual C++ runtime is installed'),
  N_('The Visual C++ runtime isn\'t installed'),
  N_('The drive is nearly full'),
  N_('The game\'s files are in its folder'),
  N_('The launcher can write to its folder'),
  N_('The program it starts isn\'t there'),
  N_('What it printed last time'),
  N_('Windows can block or slow down a program with that mark. Unblock takes it off this game\'s files.'),
  N_('Windows marked it as downloaded from the internet'),
  N_('Your copy of the game checks out'),
  N_('Your copy of the game isn\'t set yet'),
  N_('Your copy of the game isn\'t the one it reads'),
  N_('Your copy of the game isn\'t there anymore'),
  N_('Your graphics driver doesn\'t have Vulkan'),
  N_('Your own launch options are on. If it won\'t start, try once without them.'),
  N_('{error}. Games often can\'t save or set up in protected folders like Program Files.'),
  N_('{gb} GB free. Some games need room for saves and shader caches.'),
  N_('{name} is already open'),
  N_('{path} can\'t be found. Maybe it\'s on a drive that isn\'t connected, or it was moved.'),
];
function actOn(id, a) { const el = document.createElement('i'); el.dataset.id = id; el.dataset.act = a; return act(el, new Event('click')); }
const TS_ICON = { ok: '<path d="m5 12.5 4.5 4.5L19 7.5"/>', warn: '<path d="M12 8v5M12 16.5v.01"/><path d="M10.3 3.9 2.6 17.5a2 2 0 0 0 1.7 3h15.4a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z"/>', bad: '<path d="m7 7 10 10M17 7 7 17"/>', info: '<circle cx="12" cy="12" r="9"/><path d="M12 11v5M12 8v.01"/>' };
async function troubleshootModal(id, lastCode) {
  const name = (libApp(S.library[id] || { id, name: id }) || {}).name || id;
  let checks = null;
  const draw = () => {
    const box = $('#ts-list', m);
    if (!box) return;
    if (!checks) { box.innerHTML = `<p class="muted">${esc(t('Checking…'))}</p>`; return; }
    box.innerHTML = checks.map((c, i) => `<div class="ts-row ${c.level}"><span class="ts-ic"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${TS_ICON[c.level] || ''}</svg></span>
      <div class="ts-k"><b>${esc(tf(c.title, c.v))}</b>${c.detail ? `<span class="${c.title === 'What it printed last time' ? 'ts-out' : ''}">${esc(c.title === 'What it printed last time' ? c.detail : tf(c.detail, c.v))}</span>` : ''}</div>
      ${c.fix && TS_FIX[c.fix] ? `<button type="button" class="btn sm${c.level === 'bad' ? ' main' : ''}" data-x="fix" data-i="${i}">${esc(t(TS_FIX[c.fix][0]))}</button>` : ''}</div>`).join('');
  };
  const run = async () => { checks = null; draw(); try { checks = await invoke('troubleshoot', { id }); } catch (e) { checks = [{ level: 'bad', title: String(e && e.message ? e.message : e), detail: '', fix: '' }]; }
    if (lastCode) checks.push({ level: 'info', title: 'How it closed last time', detail: 'Code {code}', v: { code: lastCode }, fix: '' });
    // Problems first, what it printed last.
    const rank = (c) => (c.title === 'What it printed last time' ? 9 : { bad: 0, warn: 1, info: 2, ok: 3 }[c.level] ?? 4);
    checks = checks.map((c, i) => [c, i]).sort((x, y) => rank(x[0]) - rank(y[0]) || x[1] - y[1]).map((x) => x[0]); draw(); };
  const m = modal(`<div class="ts-h"><h2>${esc(tf('Troubleshoot {name}', { name }))}</h2><span class="bh-tag">${esc(t('Beta'))}</span></div>
    <p class="muted">${esc(t('What could keep it from starting, and a fix where there is one.'))}</p>
    <div class="ts-list" id="ts-list"></div>
    <div class="acts grid2"><button type="button" class="btn" data-x="again">${esc(t('Check Again'))}</button><button type="button" class="btn main" data-x="send">${esc(t('Send to the Developer'))}</button></div>`, async (x, close) => {
    if (x.dataset.x === 'again') return run();
    if (x.dataset.x === 'fix') { const c = checks[Number(x.dataset.i)]; close(); try { await TS_FIX[c.fix][1](id); } catch (e) { fail(e); } return; }
    if (x.dataset.x === 'send') {
      const text = (checks || []).map((c) => { const f = (x) => String(x).replace(/\{(\w+)\}/g, (k, n) => (c.v && n in c.v ? c.v[n] : k)); return `[${c.level}] ${f(c.title)}${c.detail ? ': ' + f(c.detail).replace(/\s+/g, ' ').slice(0, 300) : ''}`; }).join('\n');
      S.betaFb = Object.assign({ kind: 'bug', area: 'Playing a game', mood: '', text: '', contact: '', attach: true, sent: false, busy: false, game: '' }, S.betaFb || {}, { kind: 'bug', area: 'Playing a game', sent: false, text: (t('It won\'t start. Troubleshoot found:') + '\n' + text).slice(0, 1800) });
      S.reportGame = id;
      close();
      return setSection('report');
    }
  }, tf('Troubleshoot {name}', { name }));
  m.querySelector('.modal').classList.add('ts');
  draw();
  run();
}

// A SteamGridDB picture: { url, thumb } (older answers: just the address).
const artUrl = (x) => (typeof x === 'string' ? x : (x && x.url) || '');
const artThumb = (x) => (typeof x === 'string' ? x : (x && (x.thumb || x.url)) || '');
// Columns for a grid of n that never leave one alone on the last row.
const evenCols = (n, tries) => tries.find((c) => n <= c || n % c !== 1) || tries[0];

// Every cover and background SteamGridDB has for the game, to pick from.
function artPicker(art, cur, done) {
  const sel = { cover: cur.cover, hero: cur.hero };
  let tab = art.covers.length ? 'cover' : 'hero';
  const m = modal(`<h2>${esc(t('Choose Art'))}</h2>
    <div class="seg ap-tabs" role="radiogroup" aria-label="${esc(t('Choose Art'))}">
      <button type="button" role="radio" data-x="tab" data-k="cover">${esc(tf('Covers ({n})', { n: art.covers.length }))}</button>
      <button type="button" role="radio" data-x="tab" data-k="hero">${esc(tf('Backgrounds ({n})', { n: art.heroes.length }))}</button>
    </div>
    <div class="ap-grid" id="ap-grid"></div>
    <div class="acts grid2"><button type="button" class="btn" data-x="no">${esc(t('Cancel'))}</button><button type="button" class="btn main" data-x="use">${esc(t('Use These'))}</button></div>`, (x, close) => {
    if (x.dataset.x === 'tab') { tab = x.dataset.k; return draw(); }
    if (x.dataset.x === 'pick') { sel[tab] = x.dataset.u; return draw(); }
    if (x.dataset.x === 'use') { Object.assign(cur, sel); close(); done(); }
  }, t('Choose Art'));
  m.querySelector('.modal').classList.add('ap');
  function draw() {
    for (const b of $$('[data-x="tab"]', m)) b.setAttribute('aria-checked', String(b.dataset.k === tab));
    const list = tab === 'cover' ? art.covers : art.heroes;
    const g = $('#ap-grid', m);
    g.className = 'ap-grid ' + tab;
    g.style.setProperty('--cols', evenCols(list.length, tab === 'cover' ? [5, 4, 6, 3] : [2, 3]));
    g.innerHTML = list.length ? list.map((x) => { const u = artUrl(x); return `<button type="button" class="ap-it${u === sel[tab] ? ' on' : ''}" data-x="pick" data-u="${esc(u)}" aria-pressed="${u === sel[tab]}">${pic(artThumb(x))}</button>`; }).join('')
      : `<p class="muted">${esc(t('SteamGridDB has none of these for this game.'))}</p>`;
  }
  draw();
}

function editInfo(id) {
  const a = byId(id);
  if (!a || !a.custom) return;
  gameInfoModal({ id, info: { name: a.name, game: a.game, origin: a.origin, cover: a.cover, hero: a.hero, logo: a.logo } });
}
function renameModal(id) {
  const a = libApp(S.library[id] || { id, name: id }), names = Object.assign({}, (S.settings.ui || {}).names);
  const own = a._name !== undefined ? a._name : a.name;
  const m = modal(`<h2>${esc(t('Rename'))}</h2><p>${esc(tf('Shown everywhere in the launcher instead of "{name}". Leave it empty for its own name.', { name: own }))}</p>
    <input class="field" id="rn" type="text" value="${esc(names[id] || '')}" placeholder="${esc(own)}" spellcheck="false" autocomplete="off" maxlength="80" aria-label="${esc(t('Name'))}">
    <div class="acts"><button type="button" class="btn" data-x="no">${esc(t('Cancel'))}</button><button type="button" class="btn main" data-x="yes">${esc(t('Save'))}</button></div>`, async (x, close) => {
    if (x.dataset.x !== 'yes') return;
    const v = $('#rn', m).value.trim();
    if (v && v !== own) names[id] = v; else delete names[id];
    await saveUi({ names });
    byId(id);
    close();
    toast(v && v !== own ? t('Renamed.') : t('Back to its own name.'));
    render(true);
  }, t('Rename'));
  const f = $('#rn', m); f.focus(); f.select();
  f.addEventListener('keydown', (e) => { if (e.key === 'Enter') $('[data-x="yes"]', m).click(); });
}
function launchOptions(id) {
  const g = S.library[id];
  const m = modal(`<h2>${esc(tf('Launch options for {name}', { name: g.name }))}</h2><p>${esc(t('Added after everything else when the game starts. Put a setting with spaces in quotes.'))}</p>
    <input class="field" id="lo" type="text" value="${esc(g.args || '')}" placeholder="--example" spellcheck="false" autocomplete="off" aria-label="${esc(t('Launch options'))}">
    <div class="acts"><button type="button" class="btn" data-x="no">${esc(t('Cancel'))}</button><button type="button" class="btn main" data-x="yes">${esc(t('Save'))}</button></div>`, async (x, close) => {
    if (x.dataset.x !== 'yes') return;
    try { S.library[id] = await invoke('set_game_options', { id, args: $('#lo', m).value, autoUpdate: null }); close(); toast($('#lo', m).value.trim() ? t('Saved. They\'re used the next time it starts.') : t('Launch options cleared.')); refreshGame(); } catch (e) { fail(e); }
  }, t('Launch options'));
  const f = $('#lo', m); f.focus(); f.select();
  f.addEventListener('keydown', (e) => { if (e.key === 'Enter') $('[data-x="yes"]', m).click(); });
}

// ---------- clicks ----------
async function act(el, e) {
  const a = el.dataset.act, id = el.dataset.id;
  if (/^(acc|saves)-/.test(a) && window.accountAct) return window.accountAct(a, el);
  if (/^beta-/.test(a) && window.moreAct) return window.moreAct(a, el);
  if (a === 'adopt' && window.adoptAct) return window.adoptAct(a, el);
  switch (a) {
    case 'open': e.preventDefault(); closeMenu(); return go('game', id, el.closest('.tile') ? el : null);
    case 'back': return back();
    case 'tab': return showTab(el.dataset.tab);
    case 'slide': return showSlide(Number(el.dataset.i));
    case 'slide-step': return showSlide((S.slide || 0) + Number(el.dataset.d));
    case 'shelf': { const sh = el.closest('.shelf'); S.shelf[sh.dataset.shelf] = (S.shelf[sh.dataset.shelf] || 0) + Number(el.dataset.d); return layoutShelves(sh.parentElement); }
    case 'sort': S.find.sort = el.dataset.k; return updateDiscover();
    case 'clear-q': { S.q = ''; const q = $('#dq'); if (q) { q.value = ''; q.focus(); } return refreshList('dq'); }
    case 'pin': { const p = (S.settings.ui || {}).pins || []; const on = p.includes(id); await saveUi({ pins: on ? p.filter((x) => x !== id) : [...p, id] }); return toast(on ? t('Unpinned.') : t('Pinned to the sidebar.')); }
    case 'fav': { const f = (S.settings.ui || {}).favs || []; const on = f.includes(id); await saveUi({ favs: on ? f.filter((x) => x !== id) : [...f, id] }); toast(on ? t('Removed from Favorites.') : t('Added to Favorites.')); return render(true); }
    case 'update-all': {
      // Every game with an update, one after another (downloads wait their turn).
      const ids = Object.keys(S.updates).filter((x) => S.library[x] && !S.running.has(x) && !S.jobs[x]);
      for (const x of ids) startInstall(x);
      if (ids.length) toast(tf('Updating {n} games.', { n: ids.length }));
      return;
    }
    case 'auto-up': try { S.library[id] = await invoke('set_game_options', { id, args: null, autoUpdate: !S.library[id].auto_update }); toast(S.library[id].auto_update ? tf('{name} updates by itself now.', { name: S.library[id].name }) : t('Updates wait for you again.')); render(true); } catch (err) { fail(err); } return;
    case 'launch-opts': return launchOptions(id);
    case 'rename': return renameModal(id);
    case 'troubleshoot': return troubleshootModal(id);
    case 'edit-info': return editInfo(id);
    case 'sgdb-save': { const v = ($('#sgdb-key') || {}).value || ''; await saveUi({ sgdb_key: v.trim() }); return toast(v.trim() ? t('Saved. Games you add from GitHub get art now.') : t('Key removed.')); }
    case 'build': return changeBuild(id);
    case 'gh-add': return githubModal();
    case 'setup': return setupModal(id);
    case 'setupdate': {
      const su = S.setup[id] || {};
      const why = await pickUpdate(id, !!su.update_folder && !su.update_file);
      if (why === null) return;
      if (why) return fail(why);
      toast(t('Got it. Press Play.'));
      if (S.view === 'game' && S.arg === id && $('#g-body')) showTab(S.tab); else render(true);
      return;
    }
    case 'setupreq': return setupReqModal(id);
    case 'addlib': return addToLibrary(id);
    case 'libview': {
      const wide = !libWide();
      S.settings.ui = Object.assign({}, S.settings.ui, { lib_view: wide ? 'wide' : '' });
      saveUi({ lib_view: wide ? 'wide' : '' }, true);
      const l = $('#llist');
      if (!l) return render(true);
      const b = $('.ts-view');
      if (b) { b.innerHTML = wide ? VIEW_WIDE : VIEW_COVERS; b.title = b.ariaLabel = wide ? t('Show covers') : t('Show wide tiles'); }
      l.classList.toggle('wide', wide);
      l.innerHTML = libraryGrid();
      evenGrids(l);
      fitTitles(l);
      l.animate([{ opacity: 0, transform: 'translateY(8px)' }, { opacity: 1, transform: 'none' }], { duration: 260, easing: 'cubic-bezier(.2,.8,.2,1)' });
      return;
    }
    case 'tag': S.find = Object.assign({}, S.find, { tag: el.dataset.tag }); S.q = ''; return go('discover');
    case 'clear-tag': S.find = Object.assign({}, S.find, { tag: '' }); return render(true);
    case 'unsave': return removeFromLibrary(id);
    case 'ui-toggle': return saveUi({ [el.dataset.k]: !(S.settings.ui || {})[el.dataset.k] });
    case 'side-show': { const h = new Set((S.settings.ui || {}).side_hide || []); h.has(el.dataset.k) ? h.delete(el.dataset.k) : h.add(el.dataset.k); return saveUi({ side_hide: [...h] }); }
    case 'side-move': {
      const keys = sideSections().map((b) => b.dataset.go), i = keys.indexOf(el.dataset.k), j = i + Number(el.dataset.d);
      if (i < 0 || j < 0 || j >= keys.length) return;
      [keys[i], keys[j]] = [keys[j], keys[i]];
      return saveUi({ side_order: keys });
    }
    case 'side-reset': return saveUi({ side_order: [], side_hide: [] });
    case 'genre': e.preventDefault(); S.find.genre = el.dataset.g; if (S.view === 'discover') { S.anim = 'enter-soft'; render(true); } else go('discover'); return;
    case 'see-all': S.find.sort = el.dataset.sort; S.find.genre = ''; S.q = ''; return go('discover');
    case 'mine': S.find.mine = !S.find.mine; return updateDiscover();
    case 'libf': S.lib.filter = el.dataset.f; return updateLibrary();
    case 'ver-menu': return verMenu(el);
    case 'follow': return toggleFollow(el.dataset.dev);
    case 'play-pick': { const a = byId(id); return menu(el, [a, ...otherInstalled(a)].map((v) => ({ act: 'play', id: v.id, label: tf('Play {name}', { name: v.name }), icon: 'play' }))); }
    case 'great': S.find.great = !S.find.great; return updateDiscover();
    case 'easy-f': S.find.easy = !S.find.easy; return updateDiscover();
    case 'mp-f': S.find.mp = !S.find.mp; return updateDiscover();
    case 'self-steam': return selfToSteam(false);
    case 'run-say': return window.askRun ? window.askRun(id, (S.library[id] || {}).played_secs || 0, true) : null;
    case 'ver-go': if (S.view === 'game') return switchVersion(id); closeMenu(); return go('game', id);
    case 'ver-hide': { const p = saveUi({ verHide: !(S.settings.ui || {}).verHide }, true); refreshGame(); return p; }
    case 'install': return startInstall(id);
    case 'update-all': for (const u of Object.keys(S.updates)) if (S.library[u] && !S.jobs[u] && !S.running.has(u)) startInstall(u); return;
    case 'cancel': return invoke('cancel', { id });
    case 'play': return playGame(id);
    case 'stop': return invoke('stop', { id });
    case 'resume': return invoke('resume_game').catch(fail);
    case 'version': return startInstall(id, undefined, el.dataset.tag);
    case 'versions': if (S.view === 'game' && S.arg === id) return showTab('updates'); S.nextTab = 'updates'; return go('game', id);
    case 'mod-install': return installMod(id, el.dataset.full);
    case 'mod-src': S.modSrc[id] = el.dataset.v; return refreshGame();
    case 'gb-sort': { const g = gbState(id); g.sort = el.dataset.v; g.q = ''; return gbLoad(id); }
    case 'gb-more': return gbLoad(id, true);
    case 'gb-retry': return gbLoad(id);
    case 'gb-open': return gbOpen(id, el.dataset.mod);
    case 'mod-remove': return removeMod(id, el.dataset.full);
    case 'mods-folder': return invoke('open_mods', { id }).catch(fail);
    case 'mods': if (S.view === 'game' && S.arg === id) return showTab('mods'); S.nextTab = 'mods'; return go('game', id);
    case 'looks': if (S.view === 'game' && S.arg === id) return showTab('looks'); S.nextTab = 'looks'; return go('game', id);
    case 'news-open': S.nextTab = 'updates'; return go('game', id);
    case 'self-update': return selfUpdate();
    case 'pick-copy': return pickCopy(id, !!el.dataset.folder);
    case 'gset': return setGameSetting(id, el.dataset.k, el.dataset.i);
    case 'look': return setLook(id, el.dataset.look);
    case 'gaction': {
      const run = async () => { try { const r = await invoke('game_action', { id, action: el.dataset.k }); if (r) toast(t(r)); } catch (err) { fail(err); } };
      if (!el.classList.contains('danger')) return run();
      const what = el.closest('.set-row').querySelector('b').textContent;
      modal(`<h2>${esc(tf('Clear its {what}?', { what: what.toLowerCase() }))}</h2><p>${esc(el.closest('.set-row').querySelector('span').textContent)}</p>
        <div class="acts"><button type="button" class="btn" data-x="no">${esc(t('Keep it'))}</button><button type="button" class="btn danger" data-x="yes">${esc(t('Clear'))}</button></div>`,
        (x, close) => { close(); if (x.dataset.x === 'yes') run(); }, t('Clear'));
      return;
    }
    case 'menu': return menu(el, gameMenu(id));
    case 'folder': try { await invoke('open_folder', { id: id || null, path: el.dataset.path || null }); } catch (err) { fail(err); } return;
    case 'app-menu': try { await invoke('desktop_shortcut', { id, menu: true }); toast(tf(S.os === 'windows' ? '{name} is in your Start menu.' : '{name} is in your app menu.', { name: S.library[id].name })); } catch (err) { fail(err); } return;
    case 'desktop': try { await invoke('desktop_shortcut', { id }); toast(tf('{name} is on your desktop.', { name: S.library[id].name })); } catch (err) { fail(err); } return;
    case 'steam': return addToSteam(id, false);
    case 'shots': S.shotsGame = id || ''; return go('shots');
    case 'shots-game': S.shotsGame = el.dataset.g; S.anim = 'enter-soft'; return render(true);
    case 'shot-open': return viewShot(Number(el.dataset.i));
    case 'about': return aboutModal();
    case 'set-tab': return setSection(el.dataset.k);
    case 'devshot': return devShotView(el.dataset.id, Number(el.dataset.i));
    case 'dl-clear': return saveUi({ [el.dataset.k]: Math.floor(Date.now() / 1000) });
    case 'dl-tab': if (S.dlTab === el.dataset.tab) return; S.dlTab = el.dataset.tab; S.anim = 'enter-soft'; return render();
    case 'upd': return toggleUpd(el);
    case 'reset': return resetModal(el.dataset.k);
    case 'dev-add': return devAdd();
    case 'dev-open': S.nextTab = 'dev'; return go('game', id);
    case 'dev-check': return devCheck(id, true);
    case 'dev-starter': try { const f = await invoke('dev_starter', { id }); toast(tf('Made {file}. Fill it in, then press Check again.', { file: baseName(f) })); delete S.dev[id]; return devCheck(id, true); } catch (err) { return fail(err); }
    case 'dev-file': return invoke('open_folder', { id }).catch(fail);
    case 'dev-remove': try { await invoke('uninstall', { id, deleteFiles: false }); delete S.library[id]; delete S.dev[id]; toast(t('Taken out of your Library. Your folder is still there.')); return go('library'); } catch (err) { return fail(err); }
    case 'refresh': toast(t('Getting the newest list…')); await loadCatalog(true); await loadOwned(); loadEasy(); S.rels = {}; toast(t('Discover is up to date.')); return render(true);
    case 'tvsize': return saveSettings({ tv_size: el.dataset.k });
    case 'shots-folder': return invoke('open_shots', { id }).catch(fail);
    case 'shot-g': return viewShot(Number(el.dataset.i), (S.shots[id] || []).map((p) => ({ path: p, game: (S.library[id] || {}).name })));
    case 'shots-root': { const f = (S.allShots || [])[0]; if (f) invoke('show_screenshot', { path: f.path }).catch(fail); return; }
    case 'go-settings': return go('settings');
    case 'shot-share': try { await invoke('copy_screenshot', { path: el.dataset.path }); toast(t('Copied. Paste it into Discord, X, a message or anywhere.')); } catch (err) { fail(err); } return;
    case 'shot-show': return invoke('show_screenshot', { path: el.dataset.path }).catch(fail);
    case 'shot-del': try { await invoke('delete_screenshot', { path: el.dataset.path }); S.allShots = (S.allShots || []).filter((x) => x.path !== el.dataset.path); delete S.shots[id]; render(true); } catch (err) { fail(err); } return;
    case 'backups': return backupsModal(id);
    case 'move': return moveModal(id);
    case 'lib-default': return saveSettings({ library_dir: el.dataset.path, library_dirs: uniq([S.settings.library_dir, ...S.settings.library_dirs].filter((d) => d !== el.dataset.path)) }).then(loadLibs);
    case 'cover': {
      // The player's own picture for the game's cover (a game the launcher has no art for, or a
      // cover they like better). Tall pictures fit best.
      const p = await pickFile({ title: t('A picture for the cover'), directory: false, multiple: false, filters: [{ name: t('Pictures'), extensions: ['png', 'jpg', 'jpeg', 'webp', 'gif', 'bmp'] }] }).catch(fail);
      if (!p) return;
      try { S.library[id] = await invoke('set_cover', { id, path: typeof p === 'string' ? p : p.path }); } catch (err) { return fail(err); }
      byId(id); toast(t('Cover changed.')); return render(true);
    }
    case 'cover-reset': try { S.library[id] = await invoke('set_cover', { id, path: null }); } catch (err) { return fail(err); } byId(id); return render(true);
    case 'hero': {
      // A wide picture behind the game's page.
      const p = await pickFile({ title: t('A picture for the background'), directory: false, multiple: false, filters: [{ name: t('Pictures'), extensions: ['png', 'jpg', 'jpeg', 'webp', 'gif', 'bmp'] }] }).catch(fail);
      if (!p) return;
      try { S.library[id] = await invoke('set_cover', { id, path: typeof p === 'string' ? p : p.path, hero: true }); } catch (err) { return fail(err); }
      byId(id); toast(t('Background changed.')); return render(true);
    }
    case 'hero-reset': try { S.library[id] = await invoke('set_cover', { id, path: null, hero: true }); } catch (err) { return fail(err); } byId(id); return render(true);
    case 'start-page': { await saveUi({ start_page: el.dataset.k || '' }); return render(true); }
    case 'theme': { const k = el.dataset.k; await saveUi({ theme: k === 'light' ? 'light' : '', oled: k === 'oled' }); return render(true); }
    case 'skip-backup': return invoke('skip_backup').catch(() => {});
    case 'check-upd': {
      el.disabled = true;
      const ok = await checkUpdates(true);
      el.disabled = false;
      if (ok && !Object.keys(S.updates || {}).length) toast(t('Everything is up to date.'));
      return render(true);
    }
    case 'lib-remove': {
      // The games in it stay where they are, and in the Library; new games just don't go there.
      await saveSettings({ library_dirs: S.settings.library_dirs.filter((d) => d !== el.dataset.path) });
      if (Number(el.dataset.n)) toast(t('Removed. The games in it stay in your Library.'));
      return loadLibs();
    }
    case 'lib-add': return addLibrary().then(loadLibs);
    case 'rom-add': {
      const p = await pickFile({ title: t('A folder with your ROMs or game backups'), directory: true }).catch(fail);
      if (!p) return;
      await saveSettings({ rom_dirs: uniq([...(S.settings.rom_dirs || []), typeof p === 'string' ? p : p.path]) });
      return scanRoms();
    }
    case 'rom-remove': await saveSettings({ rom_dirs: (S.settings.rom_dirs || []).filter((d) => d !== el.dataset.path) }); return scanRoms();
    case 'rom-scan': return scanRoms();
    case 'tv': return window.enterTV && window.enterTV();
    case 'vcredist': return invoke('open_url', { url: 'https://aka.ms/vs/17/release/vc_redist.x64.exe' }).catch(fail);
    case 'report': S.reportGame = id; return setSection('report');
    case 'portable-open': return invoke('open_folder', { id: null, path: (S.portableInfo || {}).data }).catch(fail);
    case 'portable-make': {
      const f = await pickFile({ title: t('Where to put the portable copy'), directory: true }).catch(fail);
      const path = f && (typeof f === 'string' ? f : f.path);
      if (!path) return;
      try { const at = await invoke('make_portable', { target: path }); toast(tf('Portable copy made in {folder}. Start RecompLauncher from there.', { folder: at })); invoke('open_folder', { id: null, path: at }).catch(() => {}); } catch (err) { fail(err); }
      return;
    }
    case 'setexe': {
      const g = S.library[id];
      const p = await pickFile({ title: t('The program that starts the game'), defaultPath: g.path, directory: false, multiple: false, filters: S.os === 'windows' ? [{ name: t('Programs'), extensions: ['exe'] }] : [] }).catch(fail);
      if (!p) return;
      try { S.library[id] = await invoke('set_exe', { id, path: typeof p === 'string' ? p : p.path }); toast(t('Play starts that program now.')); render(true); } catch (err) { fail(err); }
      return;
    }
    case 'uninstall': return confirmUninstall(id);
    case 'url': e.preventDefault(); return invoke('open_url', { url: el.dataset.url }).catch(fail);
    case 'copy-ids': {
      const ids = S.ids || await invoke('data_ids');
      try { await navigator.clipboard.writeText(`Diagnostics ID: ${ids.usage}\nStats ID: ${ids.stats}`); toast(t('Copied.')); } catch (e) { toast(t('Couldn\'t copy.'), true); }
      return;
    }
    case 'reset-ids': {
      try { S.ids = await invoke('reset_ids'); toast(t('New IDs made')); render(true); } catch (e) { fail(e); }
      return;
    }
    case 'toggle': {
      const k = el.dataset.k;
      // Not set yet: on for handhelds, off otherwise; a press sets the opposite of what shows.
      if (k === 'minimize_playing' || k === 'lighter') { const now = S.settings[k] == null ? !!S.handheld : !!S.settings[k]; await saveSettings({ [k]: !now }); applyLighter(); return; }
      if (k === 'hide_ai' && S.view === 'discover') { saveSettings({ hide_ai: !S.settings.hide_ai }, true); return updateDiscover(); }
      // Sharing: what shows is what's sent (nothing before the player was asked).
      if (k === 'share_usage' || k === 'share_stats') return saveSettings({ [k]: !(S.settings.data_asked && S.settings[k]), data_asked: true });
      return saveSettings({ [k]: !S.settings[k] });
    }
    case 'libdir': {
      const p = await pickFile({ title: t('Where new games install'), directory: true, defaultPath: S.settings.library_dir }).catch(fail);
      if (p) saveSettings({ library_dir: typeof p === 'string' ? p : p.path });
      return;
    }
  }
}

document.addEventListener('click', (e) => {
  if (!e.target.closest('.menu')) closeMenu();
  const link = e.target.closest('[data-url]:not([data-act])');
  if (link) { e.preventDefault(); invoke('open_url', { url: link.dataset.url }).catch(fail); return; }
  const g = e.target.closest('[data-go]');
  if (g) { e.preventDefault(); if (['home', 'discover', 'library', 'achievements', 'downloads', 'shots', 'saves', 'settings'].includes(g.dataset.go) || g.dataset.go.startsWith('plug:')) { if (g.closest('.side')) { if (g.dataset.go === 'discover') S.find.genre = ''; if (g.dataset.go === 'shots') S.shotsGame = ''; } go(g.dataset.go); } return; }
  const a = e.target.closest('[data-act]');
  if (a && a.tagName !== 'SELECT') { if (a.closest('.menu')) closeMenu(); act(a, e); }
});
document.addEventListener('change', (e) => {
  const s = e.target.closest('select[data-act]');
  if (!s) return;
  if (s.dataset.act === 'gsel') return setGameSetting(s.dataset.id, s.dataset.k, s.value);
  if (s.dataset.act === 'lang') return setLanguage(s.value);
  if (s.dataset.act === 'origin') { S.find.origin = s.value; return updateDiscover(); }
  if (s.dataset.act === 'genre-sel') { S.find.genre = s.value; return updateDiscover(); }
  if (s.dataset.act === 'tag-sel') { S.find.tag = s.value; return render(true); }
  if (s.dataset.act === 'show-sel') { S.find.show = s.value; return updateDiscover(); }
  if (s.dataset.act === 'libsort') { S.lib.sort = s.value; return updateLibrary(); }
  if (s.dataset.act === 'liborigin') { S.lib.origin = s.value; return updateLibrary(); }
  render(true);
});
function focusSearch() {
  if (S.view !== 'discover' && S.view !== 'library') go('discover');
  setTimeout(() => { const q = $('#dq') || $('#lq'); if (q) { q.focus(); q.select(); } }, 60);
}
document.addEventListener('keydown', (e) => {
  // The setup dialog: Escape closes it, and Tab stays inside it.
  if (SU.el) {
    if (e.key === 'Escape') { e.preventDefault(); return closeSetup(); }
    if (e.key === 'Tab') {
      const items = $$('button:not([disabled])', SU.el);
      const i = items.indexOf(document.activeElement);
      if (items.length && (i < 0 || (e.shiftKey ? i === 0 : i === items.length - 1))) { e.preventDefault(); items[e.shiftKey ? items.length - 1 : 0].focus(); }
    }
    return;
  }
  // Something over the window (a dialog, the file browser, the setup): keys stay inside it.
  const over = [...document.querySelectorAll('.scrim, .oob')].pop();
  if (over) {
    if (e.key === 'Tab') {
      const items = $$('button:not([disabled]), input:not([disabled]), select:not([disabled]), a[href]', over).filter((x) => x.offsetParent !== null);
      const i = items.indexOf(document.activeElement);
      if (items.length && (i < 0 || (e.shiftKey ? i === 0 : i === items.length - 1))) { e.preventDefault(); items[e.shiftKey ? items.length - 1 : 0].focus(); }
    } else if (!over.contains(document.activeElement)) {
      const f = $('button:not([disabled])', over);
      if (f) f.focus();
      if (e.key === 'Escape' && e.isTrusted && over.classList.contains('scrim')) over.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    }
    return;
  }
  const typing = e.target.closest && e.target.closest('input, textarea, select');
  if ((e.ctrlKey || e.metaKey) && (e.key.toLowerCase() === 'k' || e.key.toLowerCase() === 'f')) { e.preventDefault(); focusSearch(); }
  else if (e.key === '/' && !typing) { e.preventDefault(); focusSearch(); }
  else if (e.key === 'Escape') { if ($('.menu')) closeMenu(); else if (e.target.id === 'dq' || e.target.id === 'lq') { e.target.value = ''; if (e.target.id === 'dq') S.q = ''; else S.lq = ''; refreshList(e.target.id); e.target.blur(); } }
  else if (e.altKey && e.key === 'ArrowLeft') back();
});
document.addEventListener('mouseup', (e) => { if (e.button === 3) back(); });
// Typing in a search redraws only the list under it, so the field keeps its place.
function refreshList(which) {
  const box = which === 'dq' ? $('#dlist') : $('#llist');
  if (!box) return render(true);
  morphList(box, which === 'dq' ? discoverGrid() : libraryGrid());
  if (which === 'dq') { const h = $('.dhead h1'); if (h) h.textContent = S.q.trim() ? t('Search') : S.find.genre ? t(GENRES[S.find.genre]) : t('Discover'); const c = $('.bigsearch .clear'); if (c) c.hidden = !S.q.trim(); }
}
let qTimer, tileTimer;
document.addEventListener('input', (e) => {
  const id = e.target.id;
  // The Library's tile size: live while dragging, saved once it settles.
  if (e.target.dataset && e.target.dataset.act === 'tile-size') {
    const v = Number(e.target.value);
    const l = $('#llist'); if (l) flipGrid(l, () => { l.style.setProperty('--tile', v); evenGrids(l); });
    clearTimeout(tileTimer);
    tileTimer = setTimeout(() => { S.settings.ui = Object.assign({}, S.settings.ui, { tile: v }); saveUi({ tile: v }, true); }, 400);
    return;
  }
  // Typing in Home's search box: on to Discover's search, with what's been typed so far.
  if (id === 'hq') {
    S.q = e.target.value;
    go('discover');
    setTimeout(() => { const q = $('#dq'); if (q) { q.focus(); q.setSelectionRange(q.value.length, q.value.length); } }, 30);
    return;
  }
  if (id !== 'dq' && id !== 'lq') return;
  clearTimeout(qTimer);
  qTimer = setTimeout(() => { if (id === 'dq') S.q = e.target.value; else S.lq = e.target.value; refreshList(id); }, 120);
});
$('#isl').addEventListener('click', () => go('downloads'));
// Past the top of a page, the window's buttons get a bar behind them.
$('#main').addEventListener('scroll', () => { const s = $('#main').scrollTop > 10; const bar = $('.topbar'); if (bar.classList.contains('solid') !== s) bar.classList.toggle('solid', s); }, { passive: true });
$('#collapse').addEventListener('click', toggleSide);
let mqTimer;
document.addEventListener('input', (e) => {
  if (!e.target.matches('[data-modq]')) return;
  clearTimeout(mqTimer);
  mqTimer = setTimeout(() => {
    S.modQ = e.target.value;
    const at = e.target.selectionStart;
    render(true);
    const f = $('[data-modq]');
    if (f) { f.focus(); f.setSelectionRange(at, at); }
  }, 160);
});
let fitTimer;
addEventListener('resize', () => { clearTimeout(fitTimer); fitTimer = setTimeout(() => { renderSide(); evenGrids($('#main')); layoutShelves($('#main')); fitTitles($('#main')); moveTabPill(); }, 90); });
// The sidebar's width changes the room for everything else.
$('#side').addEventListener('transitionend', (e) => { if (e.propertyName === 'width') { evenGrids($('#main')); layoutShelves($('#main')); fitTitles($('#main')); } });
if (document.fonts && document.fonts.ready) document.fonts.ready.then(() => fitTitles($('#main')));
// Pictures that don't load go away (the poster's name shows instead); a logo becomes the title.
document.addEventListener('error', (e) => {
  const img = e.target;
  if (!(img instanceof HTMLImageElement)) return;
  if (img.parentNode && img.parentNode.classList.contains('sl-logo')) return logoFail(img);
  if (img.dataset.fallback) { const h = document.createElement('h1'); h.textContent = img.alt; if (img.dataset.fallback !== '1') h.className = img.dataset.fallback; img.replaceWith(h); } else img.remove();
}, true);

// Right-click: the game's menu on a game, the screenshot's on a screenshot, a short one elsewhere.
document.addEventListener('contextmenu', (e) => {
  if (e.target.closest('input, textarea, .md, .logbox, .about p')) return;
  e.preventDefault();
  if (document.querySelector('.scrim, .viewer') || document.body.classList.contains('tv-on')) return;
  const at = { x: e.clientX, y: e.clientY };
  if (e.target.closest('#brand')) return menu(at, [{ act: 'about', label: t('About RecompLauncher'), icon: 'info' }, { act: 'refresh', label: t('Refresh'), icon: 'refresh' }]);
  const shot = e.target.closest('.shot[data-path]');
  if (shot) {
    const p = esc(shot.dataset.path);
    menu(at, [{ act: shot.dataset.act, id: shot.dataset.id || '', label: t('View'), icon: 'shot' }, { act: 'shot-share', label: t('Share (copy the picture)'), icon: 'share' }, { act: 'shot-show', label: t('Show in folder'), icon: 'folder' }, '-', { act: 'shot-del', id: shot.dataset.id || '', label: t('Delete'), cls: 'danger', icon: 'bin' }]);
    $$('.menu button').forEach((b) => { b.dataset.path = shot.dataset.path; b.dataset.i = shot.dataset.i; });
    return;
  }
  const g = e.target.closest('[data-id]');
  const id = g && g.dataset.id;
  if (id && (byId(id) || S.library[id])) return menu(at, gameMenu(id));
  menu(at, [{ act: 'refresh', label: t('Refresh'), icon: 'refresh' }, { act: 'go-settings', label: t('Settings'), icon: 'gear' }, { act: 'about', label: t('About RecompLauncher'), icon: 'info' }]);
});
$('#brand').addEventListener('click', () => aboutModal());
// Pictures fade in as they arrive instead of popping in.
document.addEventListener('load', (e) => { if (e.target instanceof HTMLImageElement) { e.target.classList.add('ld'); LOADED.add(e.target.getAttribute('src')); } }, true);
// Coming back from Big Picture mode (the window leaving full screen), a picture can be left waiting
// to load, or loaded but never shown: black covers. Each one is shown if it's in, or loaded now.
window.reviveImages = () => {
  for (const im of document.querySelectorAll('#app img')) {
    if (im.complete && im.naturalWidth > 0) { im.classList.add('ld'); LOADED.add(im.getAttribute('src')); continue; }
    if (im.loading === 'lazy') im.loading = 'eager';
    const src = im.getAttribute('src');
    if (src && im.complete) { im.removeAttribute('src'); im.setAttribute('src', src); }
  }
};

// The window has no system title bar, so these are its buttons.
const W = T.window.getCurrentWindow();
$('#w-min').addEventListener('click', () => W.minimize());
$('#w-max').addEventListener('click', () => W.toggleMaximize());
// Maximized, the middle button shows two overlapping squares (Restore), like Windows' own.
const MAX_ICON = '<svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><rect x="1.5" y="1.5" width="9" height="9" rx="2" fill="none" stroke="currentColor" stroke-width="1.3"/></svg>';
const RESTORE_ICON = '<svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><rect x="1.5" y="3.5" width="7" height="7" rx="1.6" fill="none" stroke="currentColor" stroke-width="1.3"/><path d="M3.8 1.5h5c.9 0 1.7.8 1.7 1.7v5" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/></svg>';
async function paintMaxButton() {
  let max = false;
  try { max = await W.isMaximized(); } catch (e) { /* the button stays as it is */ }
  const b = $('#w-max');
  if (!b || b.dataset.max === String(max)) return;
  b.dataset.max = String(max);
  b.innerHTML = max ? RESTORE_ICON : MAX_ICON;
  const label = max ? t('Restore') : t('Maximize');
  b.setAttribute('aria-label', label);
  b.title = label;
}
try { W.onResized(() => paintMaxButton()); } catch (e) { /* older runtime */ }
paintMaxButton();
$('#w-close').addEventListener('click', () => W.close());

T.event.listen('install', ({ payload: p }) => {
  const id = p.id;
  if (['done', 'error', 'cancelled'].includes(p.phase)) {
    delete S.jobs[id];
    ENDED[id] = Date.now();
    S.recent = [p].concat(S.recent.filter((r) => r.id !== id)).slice(0, 20);
    if (p.phase === 'done') S.dlHist = null;
    const name = byId(id)?.name || id;
    if (p.phase === 'done') {
      delete S.updates[id]; delete S.mods[id]; delete S.setup[id]; loadNews();
      // A game the launcher sets up: straight on to its copy, unless it already has one.
      const busy = SU.el || (window.tvSetupBusy && window.tvSetupBusy());
      if (S.easy.has(id) && !busy) loadSetup(id).then(() => { const su = S.setup[id]; if (su && su.needs && !su.copy) setupModal(id); else toast(tf('{name} is ready to play.', { name })); });
      else if (S.easy.has(id)) toast(tf('{name} is installed. Set it up from its page.', { name }));
      else toast(tf('{name} is ready to play.', { name }));
    }
    else if (p.phase === 'error' && String(p.msg || '').startsWith('PICK_BUILD|')) { S.recent = S.recent.filter((r) => r !== p); pickBuild(id, p.msg.split('|').slice(1)); }
    else if (p.phase === 'error') { if (!blocked(id, p.msg)) toast(tf('{name}: {msg}', { name, msg: cleanMsg(p.msg) }), true); p.msg = cleanMsg(p.msg); }
    refreshState().then(() => render(true));
    return;
  }
  const had = S.jobs[id];
  p.at = Date.now();
  S.jobs[id] = p;
  if (!had || had.phase !== p.phase) render(true); else { paintProgress(id); paintIsland(); }
});

T.event.listen('game', ({ payload: p }) => {
  if (p.state === 'running') S.running.add(p.id);
  else {
    S.running.delete(p.id);
    // 0xC000013A: its console window was closed, which is how some games are quit.
    if (p.code != null && p.code !== 0 && p.code !== -1073741510 && p.code !== 3221225786) crashModal(p);
    delete S.shots[p.id];
    // ReShade may have been downloaded for it, and there may be new screenshots to preview on.
    forgetLooks(p.id);
  }
  refreshState().then(() => render(true));
});

// A recomp's own installer or first-time setup, opened by its first Play.
// Its own setup finished, but the game it made wasn't found: the player points at it.
function lostModal(id, name) {
  const a = byId(id) || {};
  const file = (((a.boot || {}).installer || {}).then_start || '').split(/[\\/]/).pop();
  modal(`<h2>${esc(tf('Where Did You Install {name}?', { name }))}</h2>
    <p>${esc(file ? tf('Its setup is done, but the launcher couldn\'t find the game. Choose {file} in the folder you installed it to, and Play starts it from then on.', { file }) : t('Its setup is done, but the launcher couldn\'t find the game. Choose its program in the folder you installed it to, and Play starts it from then on.'))}</p>
    <div class="acts grid2"><button type="button" class="btn" data-x="no">${esc(t('Not Now'))}</button><button type="button" class="btn main" data-x="pick">${esc(t('Choose the Program'))}</button></div>`, async (x, close) => {
    close();
    if (x.dataset.x !== 'pick') return;
    const g = S.library[id] || {};
    const f = await pickFile({ title: t('The program that starts the game'), defaultPath: g.path, directory: false, multiple: false, filters: S.os === 'windows' ? [{ name: t('Programs'), extensions: ['exe'] }] : [] }).catch(fail);
    if (!f) return;
    try { S.library[id] = await invoke('set_exe', { id, path: typeof f === 'string' ? f : f.path }); toast(t('Play starts that program now.')); render(true); } catch (err) { fail(err); }
  }, t('Find the game'));
}
T.event.listen('first-setup', ({ payload: p }) => {
  const name = p.name || (S.library[p.id] || {}).name || p.id;
  if (p.state === 'open') {
    S.settingUp.add(p.id);
    toast(tf('{name} has its own setup to finish first. Follow its steps, then press Play.', { name }), false, null, 9000);
  } else {
    S.settingUp.delete(p.id);
    if (p.state === 'done') toast(tf('{name} is set up. Press Play to start it.', { name }));
    else if (p.state === 'lost') {
      if (document.body.classList.contains('tv-on')) {
        toast(tf('{name}\'s setup is done, but the launcher couldn\'t find where the game went. Pick its program here.', { name }), true, null, 9000);
        S.nextTab = 'installed'; go('game', p.id);
      } else lostModal(p.id, name);
    } else toast(tf('{name}\'s setup didn\'t finish. Press Play to open it again.', { name }), true);
  }
  refreshState().then(() => render(true));
});

T.event.listen('move', ({ payload: p }) => { if (S.moveWatch) S.moveWatch(p); });
// From the quick menu: the game's screenshots, with the game still running.
T.event.listen('goto', ({ payload: p }) => {
  if (window.tvGoto && document.body.classList.contains('tv-on')) return window.tvGoto(p.view, p.id);
  if (p.view === 'shots') { S.shotsGame = p.id || ''; go('shots'); }
});
T.event.listen('toast', ({ payload }) => toast(t(String(payload)), true));
T.event.listen('setup-required', ({ payload }) => { if (S.cat && S.cat.apps.length) { invoke('take_setup_asked').catch(() => {}); setupReqModal(String(payload)); } });
// Started again from Steam (or anything with --bigpicture) while already open.
T.event.listen('bigpicture', () => { if (window.startTV && !document.body.classList.contains('tv-on')) window.startTV(); });
// Something new from a developer the player follows: Home shows it.
T.event.listen('followed', () => { if (S.view === 'home') render(true); });
// The icon by the clock: its menu in the launcher's language, and where its items go.
const TRAY_WORDS = [N_('Open RecompLauncher'), N_('Playing'), N_('Library'), N_('Discover'), N_('Screenshots'), N_('Settings'), N_('Big Picture mode'), N_('Quit RecompLauncher'), N_('RecompLauncher is still running'), N_('It\'s by the clock. Right-click its icon to quit.')];
const trayWords = () => invoke('tray_words', { words: Object.fromEntries(TRAY_WORDS.map((k) => [k, t(k)])) }).catch(() => {});
T.event.listen('tray', ({ payload: p }) => {
  const tv = document.body.classList.contains('tv-on');
  if (p === 'bp') { if (!tv && window.enterTV) window.enterTV(); return; }
  const view = { 'go:library': 'library', 'go:discover': 'discover', 'go:shots': 'shots', 'go:settings': 'settings' }[p];
  if (!view) return;
  if (tv && window.tvGoto) return window.tvGoto(view);
  if (view === 'shots') S.shotsGame = '';
  go(view);
});
// Out of sight (minimized, or closed to the clock): animations and timers rest until it's back.
T.event.listen('away', ({ payload: away }) => { S.away = !!away; document.body.classList.toggle('away', S.away); if (!S.away) lookForLauncher(false); });
T.event.listen('new-recomp', () => loadOwned().then(() => render(true)));
T.event.listen('screenshot', ({ payload: p }) => { delete S.shots[p.id]; forgetLooks(p.id); if (S.view === 'game' && S.arg === p.id) loadShots(p.id); });

(async function start() {
  try { await refreshState(); } catch (e) { fail(e); }
  const lang = (S.settings.ui || {}).lang;
  await loadLang(lang && lang !== 'auto' ? lang : systemLang());
  tDom();
  if ((S.settings.ui || {}).start_page === 'library' && Object.keys(S.library).length) S.view = 'library';
  render();
  trayWords();
  await loadCatalog(false);
  loadCounts();
  await loadOwned();
  await loadEasy(true);
  // No connection and nothing saved from before: the Library still works.
  if (S.offline && !S.cat.apps.length && Object.keys(S.library).length) S.view = 'library';
  // The page the player picked to open on (Settings → Appearance), when nothing else was opened yet.
  if ((S.settings.ui || {}).start_page === 'library' && S.view === 'home' && Object.keys(S.library).length) S.view = 'library';
  S.anim = 'enter';
  render(true);
  if (S.settings.check_updates !== false) checkUpdates();
  applyLighter();
  // Handheld or not is worked out in the background when the launcher starts.
  setTimeout(() => invoke('is_handheld').then((h) => { S.handheld = !!h; applyLighter(); if (S.view === 'settings') render(); }).catch(() => {}), 3000);
  invoke('pc_check').then((pc) => { S.pc = pc; if (S.view === 'settings') render(true); }).catch(() => {});
  loadLibs();
  lookForLauncher(true);
  loadNews();
  // The first time: the setup, like a new console. After that, TV mode if that's how they play.
  if (document.readyState === 'loading') await new Promise((ok) => addEventListener('DOMContentLoaded', ok, { once: true }));
  if (!S.settings.setup_done && window.firstRun) window.firstRun();
  else if (window.startTV && (S.settings.start_in_tv || S.bigArg)) window.startTV();
  else setTimeout(askData, 800);
  // A shortcut asked to play a recomp that still needs setting up: its Set Up window.
  invoke('take_setup_asked').then((id) => { if (id) setTimeout(() => setupReqModal(id), window.startTV && (S.settings.start_in_tv || S.bigArg) ? 1600 : 0); }).catch(() => {});
})();
