'use strict';
// RecompLauncher's window. Everything it shows comes from the Rust side (src-tauri/src/lib.rs):
// the RecompLauncher catalog, each game's GitHub releases, and what's installed. Long jobs report back
// through two events: `install` (download progress) and `game` (a game started or closed).

const T = window.__TAURI__;
// A game of a recomp that carries several is asked about as its recomp, except for what's its own (games.js).
const invoke = (cmd, args) => { args = gameArgs(cmd, args); return cmd === 'game_setup' ? T.core.invoke(cmd, args).then(screenLabels) : T.core.invoke(cmd, args); };
// A game's settings: Match My Screen, the launcher's own last choice, in the player's language.
function screenLabels(su) {
  for (const o of (su && su.settings) || []) if (o.screen && o.choices.length) o.choices[o.choices.length - 1] = tf('Match my screen · {size}', { size: o.screen });
  return su;
}

const GENRES = { action: N_('Action'), adventure: N_('Adventure'), platformer: N_('Platformers'), racing: N_('Racing'), fighting: N_('Fighting'), shooter: N_('Shooters'), sports: N_('Sports'), rpg: N_('RPG'), puzzle: N_('Puzzle') };
const OS_NAME = { windows: 'Windows', macos: 'Mac', linux: 'Linux' };
// Text for searching: lower case and without accents, so "pokemon" finds Pokémon.
const fold = (x) => String(x || '').normalize('NFD').replace(/[\u0300-\u036f]/g, '').toLowerCase();
window.fold = fold;
// Looks run on ReShade on Windows and on vkBasalt on Linux.
window.looksHere = () => S.os === 'windows' || S.os === 'linux';
const AI_USE = { code: N_('code'), art: N_('art'), text: N_('writing'), audio: N_('voice or music') };

const S = {
  os: 'windows', arch: 'x64', version: '', site: 'https://recomplauncher.com',
  library: {}, arts: {}, settings: {}, running: new Set(), settingUp: new Set(), jobs: {}, recent: [], roms: [], owned: {}, steam: false, pc: null, libs: [], shots: {},
  cat: null, offline: false, updates: {}, rels: {}, builds: {}, reports: {}, runCounts: {}, info: {}, mods: {}, modList: {}, news: null, modQ: '', setup: {}, looks: {}, gb: {}, modSrc: {}, easy: new Set(), shelf: {}, slide: 0, lq: '',
  view: 'home', arg: null, back: [], q: '', dev: {}, setTab: 'general', endedOpen: false,
  find: { genre: '', origin: '', sort: 'popular', mine: true, great: false, easy: false, hideEnded: false },
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
// A cover at the size a tile shows it: the site's smaller copy (its thumb.php, the same one the
// website uses) for pictures in its /media folder; anything else as it is.
const THUMB_RE = /^https:\/\/(?:www\.)?recomp(?:store|launcher)\.com\/media\/([A-Za-z0-9][A-Za-z0-9._-]{0,150}\.(?:jpe?g|png|webp))$/i;
const thumbOf = (u, w = 400) => { if (!u || !S.thumbs) return u; const m = THUMB_RE.exec(webUrl(u)); return m ? `${S.thumbs}?f=${encodeURIComponent(m[1])}&w=${w}` : u; };
// The site makes covers 160, 400 or 640 pixels wide. A tile shows about 160: the smallest copy
// is exact on a normal screen and a sixth of the pixels of the 400 one; high-DPI screens keep 400.
const tileThumb = () => (devicePixelRatio || 1) > 1.1 ? 400 : 160;
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
const osIcon = (k) => `<i class="os-ico" style="--m:url(img/${k}.svg)" title="${OS_NAME[k] || k}" aria-hidden="true"></i>`;
const originName = (id) => ((S.cat && S.cat.origins) || []).find((o) => o.id === id)?.name || id || '';
const isAI = (a) => !!(a && a.ai);
// Something the player has to do by hand before the first play, from the game's boot profile.
// A community server, not the game itself: it starts the server, so its button says so.
// The profiles built into the launcher: only while the catalog doesn't carry every profile itself
// ("boot_all"), and when there's no catalog at all.
// How a game's profile did (Easy Setup, a settings change), for the admin: sent with the usage
// reports, only when the player allows them.
function profileSaid(id, step, ok, err) { invoke('profile_report', { id, step, ok, error: err ? String(err && err.message ? err.message : err) : null }).catch(() => {}); }
window.profileSaid = profileSaid;
function builtinBoot() { return (S.cat && S.cat.boot_all) ? {} : (S.bootBuiltin || {}); }
const isServer = (a) => !!(((a && a.boot) || builtinBoot()[a && a.id] || {}).server);
// A line on what the player's copy should look like, by the console it's from (How to Set Up).
function setupGuide(a) {
  const b = (a && a.boot) || builtinBoot()[a && a.id] || {};
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
const isMulti = (a) => !!((((a && a.boot) || builtinBoot()[a && a.id] || {}).multiplayer) || (a && Array.isArray(a.tags) && a.tags.some((x) => /multiplayer|co-op|split.?screen|online|netplay/i.test(String(x)))));
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
const bootNote = (a) => String(((a && a.boot) || builtinBoot()[a && a.id] || {}).player_note || '');
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
// Download speed limits the player can pick (MB a second; 0 is none), sent to the app at start and
// whenever they change.
const DL_LIMITS = [0, 1, 2, 5, 10, 25, 50];
function sendLimits() {
  const ui = S.settings.ui || {};
  return invoke('download_limits', { mb: Number(ui.dl_limit) || 0, slowPlaying: ui.dl_slow_play !== false }).catch(() => {});
}
// Once, for a sidebar saved before Beta 26: Discover goes second (after Home) and Achievements to
// the bottom, where new players have them. The player can move them again after that.
function sideOnce() {
  const ui = S.settings.ui || {};
  if (ui.side_v26) return;
  let order = Array.isArray(ui.side_order) ? ui.side_order.slice() : [];
  if (order.length) {
    order = order.filter((k) => k !== 'discover' && k !== 'achievements');
    const h = order.indexOf('home');
    order.splice(h >= 0 ? h + 1 : 0, 0, 'discover');
    order.push('achievements');
  }
  saveUi({ side_v26: true, side_order: order }, true);
}
function based(a) { return a.game && !a.parent ? `<div class="based">${DISC_S}<span>${htmlFill(t('Based on {game}'), { game: `<b>${esc(a.game)}</b>` })}${a.origin ? ` · ${esc(originName(a.origin))}` : ''}</span></div>` : ''; }
// A recomp that needs things on the PC first (Python, build tools, a long build): it says so, and
// its main button is a yellow Set Up with its developer's steps until the player says it's done.
const WRENCH = '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14.7 6.3a4 4 0 0 0 5 5L21 13l-8 8-3-3 8-8-1.3-1.3a4 4 0 0 1-5-5L9 2l3 3-2 2-3-3L5.3 5.7"/></svg>';
const needsSetup = (a) => !!(a && a.setup_required);
// A recomp that only comes as a Windows installer (the catalog's "installer"), and one whose
// downloaded installer is waiting for Set Up (a new game, or an update).
const isInstallerApp = (a) => !!(a && a.installer) && S.os === 'windows';
const installerDue = (id) => !!(S.library[id] && S.library[id].installer_setup);
// The yellow Set Up: the developer's steps not done yet, or an installer waiting to run.
const setupDue = (a) => !!a && ((needsSetup(a) && !setupDone(a.id)) || installerDue(a.id));
// Turnkey: the launcher sets it up for the player (Easy Setup, or a boot profile that puts their
// files where the game reads them), with no setup steps on its first start.
const TURNKEY_HOW = ['folder', 'flag', 'env', 'setup'];
const turnkey = (a) => !!a && !a.soon && !needsSetup(a) && (S.easy.has(a.id) || TURNKEY_HOW.includes(String((((a.boot) || builtinBoot()[a.id] || {}).files || {}).how || '')));
// Turnkey recomps first, each group in the order it had. Nothing is left out.
const turnkeyFirst = (list) => [...list.filter(turnkey), ...list.filter((a) => !turnkey(a))];
// Games added to the Library without installing them yet (like a store's library): id → when.
// Text Size: the whole window zoomed, like a browser's zoom, and kept for next time.
const UI_SCALES = [0.9, 1, 1.1, 1.25, 1.5];
const uiScale = () => { const v = Number(((S.settings && S.settings.ui) || {}).ui_scale); return UI_SCALES.includes(v) ? v : 1; };
async function setUiScale(v) {
  if (!UI_SCALES.includes(v)) return;
  try { await invoke('set_ui_scale', { scale: v }); } catch (e) { return fail(e); }
  await saveUi({ ui_scale: v }, true);
  render(true);
}
// Ctrl with + or - steps through the sizes, Ctrl+0 goes back to 100%.
addEventListener('keydown', (e) => {
  if (!(e.ctrlKey || e.metaKey) || e.altKey || !S.settings) return;
  const i = UI_SCALES.indexOf(uiScale());
  const to = ['+', '='].includes(e.key) ? UI_SCALES[Math.min(i + 1, UI_SCALES.length - 1)] : e.key === '-' ? UI_SCALES[Math.max(i - 1, 0)] : e.key === '0' ? 1 : null;
  if (to == null) return;
  e.preventDefault();
  if (to !== uiScale()) setUiScale(to);
});
const saved = () => ((S.settings && S.settings.ui) || {}).saved || {};
const isSaved = (id) => !!saved()[id];
const PLUS = '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14"/></svg>';
async function addToLibrary(id) {
  // Added by the player: it stays, whatever a scan finds later.
  const mine = (((S.settings.ui || {}).found_saved) || []).filter((x) => x !== id);
  const skip = (((S.settings.ui || {}).found_skip) || []).filter((x) => x !== id);
  await saveUi({ saved: Object.assign({}, saved(), { [id]: Math.floor(Date.now() / 1000) }), found_saved: mine, found_skip: skip }, true);
  libSend(id, true);
  const a = byId(id);
  render(true);
  toast(tf('{name} is in your Library. Install it whenever you like.', { name: a ? a.name : id }));
}
async function removeFromLibrary(id) {
  const s2 = Object.assign({}, saved());
  delete s2[id];
  // A look at the player's games doesn't put it back.
  const skip = [...new Set([...(((S.settings.ui || {}).found_skip) || []), id])];
  await saveUi({ saved: s2, found_skip: skip }, true);
  libSend(id, false);
  // One added by its link goes away altogether.
  const a = byId(id);
  if (a && a.custom && !S.library[id]) { await invoke('remove_github', { id }).catch(() => {}); if (S.arts) delete S.arts[id]; await loadCatalog(false); }
  render(true);
}
// The saved ("Not installed") list goes with the account when signed in, the same list as the
// website's library: merged with this PC's on sign-in, each save and unsave sent as it happens
// (kept for later while the website can't be reached), and read again when the launcher opens and
// every 10 minutes. Signed out, it's this PC's own list. Installed games always stay per PC.
const accountId = () => (S.account && S.account.signedIn && S.account.user ? String(S.account.user.uid || S.account.user.username || '') : '');
async function libSend(id, on) {
  if (!accountId()) return;
  try { await invoke('library_push', { games: [id], on }); } catch (e) { libQueue([{ id, on }]); }
}
async function libQueue(items) {
  const ui = (S.settings && S.settings.ui) || {};
  const q = (ui.lib_queue || []).filter((x) => !items.some((y) => y.id === x.id)).concat(items);
  await saveUi({ lib_queue: q }, true);
}
async function libPushAll(ids, on) {
  for (let i = 0; i < ids.length; i += 500) await invoke('library_push', { games: ids.slice(i, i + 500), on });
}
async function libSync() {
  const uid = accountId();
  if (!uid || (S.account && S.account.offline) || S.libSyncing) return;
  S.libSyncing = true;
  try {
    const ui = (S.settings && S.settings.ui) || {};
    // What waited while the website couldn't be reached goes first.
    const q = Array.isArray(ui.lib_queue) ? ui.lib_queue : [];
    if (q.length) {
      await libPushAll(q.filter((x) => x.on).map((x) => x.id), true);
      await libPushAll(q.filter((x) => !x.on).map((x) => x.id), false);
      await saveUi({ lib_queue: [] }, true);
    }
    const r = await invoke('library_pull');
    const now = Math.floor(Date.now() / 1000);
    const remote = Object.fromEntries(((r && r.saved) || []).filter((x) => x && typeof x.id === 'string').map((x) => [x.id, Number(x.at) || now]));
    let next = remote;
    if (ui.lib_synced !== uid) {
      // The first time on this account: both lists together, and this PC's own go up.
      const local = saved();
      const up = Object.keys(local).filter((id) => !remote[id]);
      if (up.length) await libPushAll(up, true);
      next = Object.assign({}, remote, local);
    }
    const before = Object.keys(saved()).sort().join('|');
    await saveUi({ saved: next, lib_synced: uid }, true);
    if (Object.keys(next).sort().join('|') !== before && ['library', 'home'].includes(S.view)) render(true);
  } catch (e) { /* the website can't be reached: this PC's list stays, and it's tried again later */ } finally { S.libSyncing = false; }
}
setInterval(() => { if (!document.hidden) libSync(); }, 10 * 60 * 1000);
const setupDone = (id) => !!(((S.settings && S.settings.ui) || {}).setup_ok || {})[id];
const GITHUB_S = '<svg width="15" height="15" viewBox="0 0 24 24" aria-hidden="true"><path fill="currentColor" d="M12 1.8a10.2 10.2 0 0 0-3.2 19.9c.5.1.7-.2.7-.5v-1.8c-2.8.6-3.4-1.2-3.4-1.2-.5-1.2-1.1-1.5-1.1-1.5-.9-.6.1-.6.1-.6 1 .1 1.6 1 1.6 1 .9 1.6 2.4 1.1 3 .9.1-.7.4-1.1.6-1.4-2.3-.3-4.6-1.1-4.6-5 0-1.1.4-2 1-2.7-.1-.3-.4-1.3.1-2.7 0 0 .9-.3 2.8 1a9.6 9.6 0 0 1 5.1 0c1.9-1.3 2.8-1 2.8-1 .6 1.4.2 2.4.1 2.7.6.7 1 1.6 1 2.7 0 3.9-2.4 4.7-4.6 5 .4.3.7.9.7 1.9v2.8c0 .3.2.6.7.5A10.2 10.2 0 0 0 12 1.8z"/></svg>';
// Where a recomp lives: GitHub, GitLab for a project kept as gitlab:group/project, or SourceForge
// for sourceforge:<name>.
const isLab = (repo) => /^gitlab:/i.test(String(repo || ''));
const isSF = (repo) => /^sourceforge:/i.test(String(repo || ''));
const sfName = (repo) => String(repo).slice(12);
const hostName = (repo) => (isSF(repo) ? 'SourceForge' : isLab(repo) ? 'GitLab' : 'GitHub');
const repoHome = (repo) => isSF(repo) ? `https://sourceforge.net/projects/${sfName(repo)}/` : isLab(repo) ? `https://gitlab.com/${String(repo).slice(7)}` : `https://github.com/${repo}`;
const repoIssues = (repo) => isSF(repo) ? `https://sourceforge.net/projects/${sfName(repo)}/support` : isLab(repo) ? `${repoHome(repo)}/-/issues/new` : `https://github.com/${repo}/issues`;
const repoReadme = (repo) => isSF(repo) || isLab(repo) ? repoHome(repo) : `https://github.com/${repo}#readme`;
const repoFiles = (repo) => isSF(repo) ? `https://sourceforge.net/projects/${sfName(repo)}/files/` : isLab(repo) ? `${repoHome(repo)}/-/releases` : `https://github.com/${repo}/releases`;
// Where Report a problem goes: the catalog's own link when it gives one (https), else the project's.
const reportUrl = (a) => (a && /^https:\/\//i.test(String(a.report_url || '')) ? String(a.report_url) : repoIssues(a.repo));
window.isSF = isSF;
// A SourceForge project's two links: its page and its files (its releases).
const sfMenu = (repo) => [{ act: 'url', url: repoHome(repo), label: t('View on SourceForge'), icon: 'web' }, { act: 'url', url: repoFiles(repo), label: t('Releases'), icon: 'page' }];
window.repoFiles = repoFiles;
// A Coming soon game announced before it has a project: the post to follow it on (https only), and
// the button's name for it.
const followLink = (a) => (a && !a.repo && /^https:\/\/[^\s/]+\//i.test(String(a.follow || '')) ? String(a.follow) : '');
const onX = (url) => /^https:\/\/(?:[a-z0-9-]+\.)?(?:x|twitter)\.com\//i.test(String(url || ''));
const followLabel = (url) => (onX(url) ? t('Follow It on X') : t('Follow It'));
window.onX = onX;
window.followLink = followLink;
window.followLabel = followLabel;
const X_S = '<svg width="18" height="18" viewBox="0 0 24 24" aria-hidden="true"><path fill="currentColor" d="M17.8 3h3.1l-6.8 7.7L22 21h-6.2l-4.9-6.3L5.3 21H2.2l7.3-8.3L2 3h6.4l4.4 5.8L17.8 3zm-1.1 16.2h1.7L7.4 4.7H5.6l11.1 14.5z"/></svg>';
// A link to a project: GitHub's mark for GitHub, a plain branch for anywhere else.
const BRANCH_S = '<svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><circle cx="6" cy="5" r="2.2"/><circle cx="6" cy="19" r="2.2"/><circle cx="18" cy="8" r="2.2"/><path d="M6 7.2v9.6M18 10.2c0 4-6 3.4-11 6.6"/></svg>';
window.repoHome = repoHome;
window.repoReadme = repoReadme;
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
  // Asks the site once a minute at most; a second press within it reads the answer just given.
  const ask = Date.now() - (S.selfCheckedAt || 0) >= 60e3;
  if (ask) S.selfCheckedAt = Date.now();
  try { v = await invoke('launcher_update', { force: ask }); } catch { bad = true; }
  if (!v && offNow()) bad = true;
  await new Promise((r) => setTimeout(r, Math.max(0, 900 - (Date.now() - t0))));
  if (btn) btn.disabled = false;
  if (v) { el.remove(); return updateToast(v); }
  el.classList.add('found');
  el.classList.toggle('bad', bad);
  el.innerHTML = bad ? `<span>${esc(t('Couldn\'t check for updates right now.'))}</span>` : `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m5 12.5 4.5 4.5L19 7.5"/></svg><span>${esc(t('You have the newest RecompLauncher.'))}</span>`;
  setTimeout(() => { el.classList.add('out'); setTimeout(() => el.remove(), 260); }, 3600);
}
const fail = (e) => toast(t(String(e && e.message ? e.message : e).replace(/^[A-Z_]+\|/, '')), true);

// Markdown from README sections and release notes, made safe: HTML tags dropped, everything escaped,
// links opened in the browser.
function md(src, githubMentions = false) {
  const inline = (s) => s
    .replace(/!\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/`([^`]+)`/g, '<code>$1</code>')
    .replace(/\*\*([^*]+)\*\*/g, '<b>$1</b>').replace(/__([^_]+)__/g, '<b>$1</b>')
    .replace(/(^|[\s(])\*([^*\s][^*]*)\*/g, '$1<i>$2</i>').replace(/(^|[\s(])_([^_\s][^_]*)_(?=[\s.,)!?]|$)/g, '$1<i>$2</i>')
    .replace(/\[([^\]]+)\]\((https?:\/\/[^)\s]+)\)/g, '<a href="#" data-url="$2">$1</a>')
    .replace(/(^|[\s(])(https?:\/\/[^\s<)]+[^\s<).,;:!?])/g, '$1<a href="#" data-url="$2">$2</a>')
    // Link GitHub handles only in text, never inside an existing link, code or HTML attribute.
    .split(/(<a\b[^>]*>[\s\S]*?<\/a>|<code>[\s\S]*?<\/code>|<[^>]*>)/g)
    .map((part, i) => !githubMentions || i % 2 ? part : part.replace(/(^|[\s([{,:;!?])@([a-z\d](?:[a-z\d-]{0,37}[a-z\d])?)(?![\w\/-])/gi,
      '$1<a href="#" class="md-mention" data-url="https://github.com/$2">@$2</a>')).join('');
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
// Puts back what an earlier fit changed (writes only), then measures every title at once (one
// layout for all of them), and fits only the ones that don't fit.
function fitBatch(els) {
  for (const el of els) {
    if (el.dataset.full && el.textContent !== el.dataset.full) el.textContent = el.dataset.full;
    if (el.style.fontSize) el.style.fontSize = '';
    if (el.classList.contains('long')) el.classList.remove('long');
  }
  const over = els.filter((el) => { const w = el.clientWidth; return w && el.scrollWidth > w + 0.5; });
  for (const el of over) fitOne(el);
}
// A big list's tiles off screen aren't drawn until they come near (content-visibility), and
// measuring one makes it draw: their titles are fitted as they come near instead.
const FIT_LATER = new Set();
const FIT_IO = typeof IntersectionObserver === 'function' ? new IntersectionObserver((ents) => {
  const els = [];
  for (const e of ents) {
    if (!e.isIntersecting) continue;
    FIT_IO.unobserve(e.target);
    FIT_LATER.delete(e.target);
    const t = e.target.querySelector('.t');
    if (t) els.push(t);
  }
  if (els.length) fitBatch(els);
}, { rootMargin: '600px 0px' }) : null;
function fitTitles(root = document) {
  for (const tile of FIT_LATER) if (!tile.isConnected) { FIT_IO.unobserve(tile); FIT_LATER.delete(tile); }
  const els = $$('.tile .t', root);
  if (FIT_IO && els.length > 40) {
    // A big list: measuring each tile here would draw every off-screen one (content-visibility),
    // which is the very cost this avoids. They're fitted as they come near instead.
    for (const el of els) {
      const tile = el.closest('.tile');
      if (tile && !FIT_LATER.has(tile)) { FIT_LATER.add(tile); FIT_IO.observe(tile); }
    }
  } else fitBatch(els);
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
const visible = () => { const hid = hiddenSet(); return all().filter((a) => !a.custom && !(S.settings.hide_ai && isAI(a)) && !hid.has(a.id)); };
// Games the player hid from Discover and Home (a game's versions all go together). Their Library
// entries and pages stay; Settings → Discover shows them again.
const HID = { src: null, set: new Set() };
function hiddenSet() {
  const h = (S.settings && S.settings.ui && S.settings.ui.hidden) || [];
  if (HID.src !== h) { HID.src = h; HID.set = new Set(h); }
  return HID.set;
}
const isHidden = (id) => hiddenSet().has(id);
async function hideGame(id) {
  const g = verGroup(byId(id)), ids = g ? g.list.map((v) => v.id) : [id];
  await saveUi({ hidden: [...new Set([...hiddenSet(), ...ids])] }, true);
  toast(t('Hidden from Discover and Home. Settings → Discover shows it again.'));
  render(true);
}
async function unhideGame(id) {
  const g = verGroup(byId(id)), ids = new Set(g ? g.list.map((v) => v.id) : [id]);
  await saveUi({ hidden: [...hiddenSet()].filter((x) => !ids.has(x)) }, true);
  render(true);
}
window.hideGame = hideGame; window.unhideGame = unhideGame; window.isHidden = isHidden;
// Settings → Discover: the hidden games, each with a way back.
function hiddenModal() {
  const draw = () => {
    const seen = new Set(), list = [];
    for (const id of hiddenSet()) { const a = byId(id); if (!a) continue; const g = verGroup(a), k = g ? 't:' + g.id : a.id; if (seen.has(k)) continue; seen.add(k); list.push(g ? leadOf(g) : a); }
    return `<h2>${esc(t('Hidden Games'))}</h2><p>${esc(t('These don\'t show in Discover or on Home. Ones in your Library stay there.'))}</p>
      ${list.length ? `<div class="choices">${list.map((a) => `<div class="choice"><span><b>${esc(a.game || a.name)}</b><small>${esc(originName(a.origin))}</small></span><button type="button" class="btn sm" data-x="show" data-id="${esc(a.id)}">${esc(t('Show Again'))}</button></div>`).join('')}</div>` : `<p class="muted">${esc(t('Nothing is hidden.'))}</p>`}
      <div class="acts">${list.length > 1 ? `<button type="button" class="btn" data-x="all">${esc(t('Show All Again'))}</button>` : ''}<button type="button" class="btn main" data-x="no">${esc(t('Done'))}</button></div>`;
  };
  modal(draw(), async (x, close) => {
    const box = x.closest('.modal');
    if (x.dataset.x === 'show') { await unhideGame(x.dataset.id); if (box) box.innerHTML = draw(); }
    else if (x.dataset.x === 'all') { await saveUi({ hidden: [] }, true); render(true); close(); }
  }, t('Hidden Games'));
}
// Looked up by id, and grouped by game: the catalog's titles put the recomps of one game together
// as its versions, the recommended one first, then the most downloaded.
let IDX = { key: '', cat: null, ids: new Map(), titles: new Map() };
function index() {
  const apps = all();
  const key = apps.length + '|' + Object.keys(S.library).length + '|' + Object.keys(S.builds || {}).length + '|' + (S.gameSetsV || 0);
  if (IDX.cat !== S.cat || IDX.key !== key) {
    const ids = new Map(apps.map((a) => [a.id, a]));
    // Each game of a recomp that carries several, by its full id (not in Discover's list).
    addGameApps(ids);
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
// The pictures Customize changes, and where a catalog object keeps its own while another shows.
const ART_KINDS = [['cover', '_cov'], ['hero', '_hero'], ['logo', '_logo'], ['icon', '_icon']];
// What a game's versions share: the name, description and pictures the player set for every
// version (Customize's "every version" box) are kept under this key, so versions the catalog adds
// later look the same too.
const gameKey = (a) => (a && a.title ? 'game--' + a.title : '');
// A version's own picture of a kind, as the catalog has it, whatever shows in its place.
function catalogArt(a, k) {
  const stash = ART_KINDS.find((x) => x[0] === k)[1];
  return a[stash] !== undefined ? a[stash] : a[k] || '';
}
// What shows for a kind when this version has no picture of its own: the one the player set for
// every version, else (Show Game Media) the recommended version's, else the catalog's.
function usualArt(a, k) {
  const shared = (S.arts || {})[gameKey(a)] || {};
  if (shared[k + '_file']) return 'local:' + shared[k + '_file'];
  const g = (S.settings.ui || {}).game_media ? verGroup(a) : null;
  const lead = g && g.pick !== a.id ? index().ids.get(g.pick) : null;
  return catalogArt(lead || a, k);
}
// A cover the player picked for a game (Change Cover) shows instead of the catalog's, and a name
// they gave it (Rename) instead of its own.
function applyCover(a, id) {
  if (!a) return a;
  const ui = S.settings.ui || {};
  const names = ui.names || {}, gk = gameKey(a);
  // Show Game Names: the game's own name instead of the recomp's ("Tears of the Kingdom", not
  // "TotK Recomp"), unless the player named it themselves.
  const nm = names[id] || (gk && names[gk]) || (ui.game_names && a.game ? a.game : '');
  if (nm) { if (a._name === undefined) a._name = a.name; a.name = nm; } else if (a._name !== undefined) { a.name = a._name; delete a._name; }
  // On the installed game, or kept for one that isn't (Customize from Discover, or uninstalled).
  const own = S.library[id] || (S.arts || {})[id] || {};
  for (const [k, stash] of ART_KINDS) {
    const f = own[k + '_file'];
    const native = catalogArt(a, k);
    const want = f ? 'local:' + f : usualArt(a, k);
    if (want === native) { if (a[stash] !== undefined) { a[k] = a[stash]; delete a[stash]; } continue; }
    if (a[stash] === undefined) a[stash] = a[k] || '';
    a[k] = want;
  }
  return a;
}
const byId = (id) => applyCover(index().ids.get(id), id);
// A game in the Library the catalog doesn't have (added by the player, a local build).
const libApp = (g) => byId(g.id) || { id: g.id, name: ((S.settings.ui || {}).names || {})[g.id] || ((S.settings.ui || {}).game_names && g.game) || g.name, game: g.game || '', repo: g.repo, systems: [S.os], cover: g.cover_file ? 'local:' + g.cover_file : '', hero: g.hero_file ? 'local:' + g.hero_file : '', logo: g.logo_file ? 'local:' + g.logo_file : '', icon: g.icon_file ? 'local:' + g.icon_file : '' };
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
// Recomps that put out a new version lately (not just their first one), each game once.
function recentlyUpdated(list) {
  const seen = new Set();
  return list.filter((a) => notPC(a) && a.released && (Number(a.releases) || (a.releases || []).length || 0) > 1 && a.released !== a.first)
    .sort((x, y) => String(y.released).localeCompare(String(x.released)) || byDl(x, y))
    .filter((a) => { const k = a.title && index().titles.has(a.title) ? 't:' + a.title : a.id; if (seen.has(k)) return false; seen.add(k); return true; });
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
// A Windows build to fall back on (through Proton on Linux).
const hasWindowsBuild = (a) => (a.systems || []).includes('windows') || !!(a.assets && a.assets.windows);
// Linux, Proton off (or none on this PC): turned on, with GE-Proton first when nothing can run
// Windows programs yet. Then the recomp can be added and installed like any other.
async function useProton(id) {
  try {
    const now = S.protonNow || await invoke('proton_now').catch(() => null);
    S.protonNow = now;
    if (!now || !now.runner) {
      toast(t('Getting GE-Proton first. It takes a minute.'));
      await geInstall();
      if (!(S.protonNow || {}).runner) return;
    }
    await saveSettings({ proton: true });
    toast(t('Proton is on. Windows recomps play through it now.'));
    if (S.view === 'game') refreshGame(); else render(true);
  } catch (e) { fail(e); }
}
// The systems a recomp has a build for, in a fixed order.
const runsOn = (a) => ['windows', 'linux', 'macos'].filter((k) => (a.systems || []).includes(k) || !!(a.assets && a.assets[k]) || (k === S.os && !!S.builds[a.id]));
// Runs On. On Linux: native builds, Linux or Proton, or Windows only, kept between launches.
const RUNS_LINUX = [['', N_('Any')], ['linux', N_('Linux (native)')], ['proton', N_('Linux or Proton')], ['winonly', N_('Windows only')]];
const RUNS_OTHER = [['', N_('Any')], ['windows', 'Windows'], ['linux', 'Linux'], ['macos', 'Mac']];
const runsList = () => (S.os === 'linux' ? RUNS_LINUX : RUNS_OTHER);
const runsPick = () => (S.os === 'linux' ? String((S.settings.ui || {}).runs_linux || '') : S.find.runs || '');
function runsFits(a, k) {
  if (!k) return true;
  const on = runsOn(a);
  if (k === 'proton') return on.includes('linux') || on.includes('windows');
  if (k === 'winonly') return on.includes('windows') && !on.includes('linux');
  return on.includes(k);
}
async function setRuns(k) {
  if (S.os === 'linux') await saveUi({ runs_linux: k }, true); else S.find.runs = k;
}
// Small marks for those under a cover. On Linux, a Windows-only one says it plays through Proton.
function osMarks(a) {
  const on = runsOn(a);
  if (!on.length) return '';
  const px = S.os === 'linux' && !on.includes('linux') && on.includes('windows');
  return `<span class="osm${px ? ' px' : ''}" title="${esc(px ? t('Windows version, through Proton') : on.map((k) => OS_NAME[k]).join(' · '))}">${on.map(osIcon).join('')}${px ? `<b>${esc(t('Proton'))}</b>` : ''}</span>`;
}
const byDl = (x, y) => (y.downloads || 0) - (x.downloads || 0);
const byNew = (x, y) => String(y.added || y.released || '').localeCompare(String(x.added || x.released || ''));
// Old PC games and their open-source remakes (origin "pc"): their download counts go back years,
// so the rows ranked by downloads keep to console recomps, or put them first.
const isPC = (a) => !!a && a.origin === 'pc';
const notPC = (a) => !isPC(a);
const byDlConsolesFirst = (x, y) => (isPC(x) ? 1 : 0) - (isPC(y) ? 1 : 0) || byDl(x, y);
// How many recomps a list holds: every version, not every tile (a game with three versions is
// one tile and three recomps). Coming Soon ones aren't counted.
const recompCount = (list) => list.filter((a) => !a.soon).length;
const byUpdated = (x, y) => String(y.released || '').localeCompare(String(x.released || ''));
// Discover's Show filter: everything, what's installed, what isn't, or what needs setting up first.
const SHOWS = [['', N_('Everything')], ['inst', N_('Installed')], ['notinst', N_('Not installed')], ['lib', N_('In your Library')], ['setup', N_('Setup Required')], ['handheld', N_('Great on Handhelds')]];
// Several handheld players say it runs well (the catalog's "handheld", worked out by the site).
const greatHandheld = (a) => !!(a && a.handheld && a.handheld.ok);
window.greatHandheld = greatHandheld;
const HH_ICON = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><rect x="2.5" y="6.5" width="19" height="11" rx="3.5"/><path d="M7 10.5v3M5.5 12h3"/><circle cx="16.5" cy="11" r=".8" fill="currentColor"/><circle cx="18" cy="13" r=".8" fill="currentColor"/></svg>';
function showOk(a, k) {
  if (!k) return true;
  if (k === 'inst') return !!S.library[a.id];
  if (k === 'notinst') return !S.library[a.id];
  if (k === 'lib') return !!S.library[a.id] || isSaved(a.id);
  if (k === 'setup') return needsSetup(a) && !setupDone(a.id);
  if (k === 'handheld') return greatHandheld(a);
  return true;
}

function status(a) {
  // One game of several: its install, downloads and updates are the recomp's.
  const id = a.id, rid = parentId(id), job = S.jobs[rid], inst = S.library[id];
  if (S.running.has(id)) return 'running';
  if (S.settingUp.has(id) || S.settingUp.has(rid)) return 'settingup';
  if (job) return 'busy';
  if ((S.paused || {})[rid]) return 'paused';
  if (inst && inst.installer_setup) return 'installed';
  if (inst && S.updates[rid]) return 'update';
  if (inst) return 'installed';
  if (a.soon) return 'soon';
  if (!forOS(a)) return 'nobuild';
  return 'get';
}

// ---------- pieces ----------
// Where a recomp's project stands, from the catalog: ended (or archived), on hold, or quiet (no new
// release in 60 days or more). Coming Soon recomps never get one. Null when there's nothing to say.
function projectState(a) {
  if (!a || a.soon) return null;
  // Finished: everything it set out to do is in. Never shown as ended or quiet, however old its
  // last release is, and it wins over an old catalog's "archived".
  if (a.status === 'complete') return { kind: 'complete', note: a.status_note || t('The developer considers it finished: everything it set out to do is in. Fixes may still come now and then.') };
  const remote = (S.rels[a.repo] || {}).archived;
  const archived = typeof a.archived === 'boolean' ? a.archived : typeof remote === 'boolean' ? remote : a.github_archived === true;
  const ended = a.status === 'ended' || a.status === 'abandoned';
  if (ended || archived) return { kind: 'ended', note: a.status_note || (ended ? t('The developer isn\'t planning any more updates.') : t('The developer archived this project, so no more updates are coming.')) };
  if (a.status === 'paused') return { kind: 'paused', note: a.status_note || t('The developer has put it on hold. Updates may pick up again later.') };
  const r = Date.parse(a.released || '');
  if (!r) return null;
  const days = (Date.now() - r) / 864e5;
  if (days < 60) return null;
  const months = Math.floor(days / 30.4);
  return { kind: 'quiet', label: months >= 24 ? tn(Math.floor(months / 12), 'No updates in 1 year', 'No updates in {n} years') : months >= 12 ? t('No updates in over a year') : tn(months, 'No updates in 1 month', 'No updates in {n} months') };
}
// Ended means archived or abandoned. Discover's "Hide ended" switch takes those out of the list
// and puts them in the "Ended projects" box under it.
function isEnded(a) {
  return projectState(a)?.kind === 'ended';
}
function keepEnded(a, f) {
  return !f.hideEnded || !isEnded(a);
}
const STATE_NAME = { ended: N_('Ended'), paused: N_('On hold'), complete: N_('Feature complete') };
const STATE_PILL = { ended: N_('Project ended'), paused: N_('On hold'), complete: N_('Feature complete') };
// A check mark before "Feature complete", on the page and in its notice.
const STATE_ICON = { complete: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m5 12.5 4.5 4.5L19 7.5"/></svg>' };
// The page's pill for it, and (ended or on hold) the notice by Play/Install.
function statePill(a) {
  const ps = projectState(a);
  if (!ps) return '';
  return ps.kind === 'quiet' ? `<span class="tag quiet" title="${esc(t('No new release in a while. It may still get updates.'))}">${esc(ps.label)}</span>`
    : `<span class="tag ${ps.kind}">${STATE_ICON[ps.kind] || ''}${esc(t(STATE_PILL[ps.kind]))}</span>`;
}
// The site found the installed version (or the update on offer) crashing for a lot of players: say
// so by Play, with the version to go back to when there is one.
function badNote(a) {
  const g = S.library[a.id];
  if (!g) return '';
  const now = badVersion(a, g.tag), next = S.updates[a.id] && badVersion(a, S.updates[a.id]);
  if (now) {
    const back = now.use || g.prev_tag || '';
    return `<div class="gp-note bad-ver" role="note"><b>${esc(t('This Version Is Crashing a Lot'))}</b><span dir="auto">${esc(now.note || (back ? tf('{version} is crashing for a lot of players. {old} works for most.', { version: g.tag, old: back }) : tf('{version} is crashing for a lot of players. A fix may be on its way.', { version: g.tag })))}</span>${back && verKey(back) !== verKey(g.tag) ? `<button type="button" class="btn sm" data-act="goback" data-id="${esc(a.id)}" data-tag="${esc(back)}">${esc(tf('Go Back to {version}', { version: back }))}</button>` : ''}</div>`;
  }
  if (next) return `<div class="gp-note bad-ver" role="note"><b>${esc(t('The Update Is Crashing a Lot'))}</b><span dir="auto">${esc(next.note || tf('{version} is crashing for a lot of players. It\'s fine to wait for the next one.', { version: S.updates[a.id] }))}</span></div>`;
  return '';
}
// A listing the catalog replaced ("replaced": {old id: new id}): the new one, when this game is
// installed under the old id and the new one is listed. Nothing moves until the player says so.
function replacedBy(id) {
  const r = S.cat && S.cat.replaced && typeof S.cat.replaced === 'object' ? S.cat.replaced : null;
  const to = r && typeof r[id] === 'string' ? r[id] : '';
  const g = S.library[id];
  return to && to !== id && g && !g._sub && !g.local && !S.library[to] ? byId(to) || null : null;
}
function replacedNote(a) {
  const to = replacedBy(a.id);
  if (!to) return '';
  return `<div class="gp-note moved" role="note"><b>${esc(tf('This listing was replaced by {name}', { name: to.name }))}</b><span dir="auto">${esc(t('Move to it to keep getting updates. Your saves, play time and game files come along.'))}</span><button type="button" class="btn sm main" data-act="move-listing" data-id="${esc(a.id)}" data-to="${esc(to.id)}">${esc(t('Move to It'))}</button></div>`;
}
async function moveListing(from, to) {
  if (S.running.has(from) || S.jobs[from]) return toast(t('Close the game and wait for its downloads first.'), true);
  try {
    const r = await invoke('move_listing', { from, to });
    for (const cache of [S.setup, S.mods, S.modList, S.info, S.builds, S.reports, S.updates]) { delete cache[from]; delete cache[to]; }
    await refreshState();
    if (S.arg === from) S.arg = to;
    render(true); renderSide();
    if (window.tvRefresh) window.tvRefresh();
    const name = (byId(to) || {}).name || to;
    if (r && r.reinstall) { toast(tf('Moved to {name}. Its release goes in the same folder now, and your saves are backed up first.', { name })); startInstall(to); }
    else toast(tf('Moved to {name}. Nothing else changed.', { name }));
  } catch (e) { fail(e); }
}
// The catalog now installs this game with its installer, but this copy came from the old download
// (a zip without the parts the installer adds): offer to install it the new way.
function reinstallNote(a) {
  const g = S.library[a.id];
  if (!g || !isInstallerApp(a) || g.installer || g.installer_setup || S.jobs[a.id]) return '';
  return `<div class="gp-note bad-ver" role="note"><b>${esc(t('This Game Installs Differently Now'))}</b><span dir="auto">${esc(t('It comes as an installer now, which brings parts your copy doesn\'t have. Reinstall it to get them. Your saves stay.'))}</span><button type="button" class="btn sm" data-act="reinstall-new" data-id="${esc(a.id)}">${esc(t('Reinstall'))}</button></div>`;
}
function stateNote(a) {
  const ps = projectState(a);
  return ps && ps.kind !== 'quiet' ? `<div class="gp-note ${ps.kind}" role="note"><b>${STATE_ICON[ps.kind] || ''}${esc(t(STATE_PILL[ps.kind]))}</b><span dir="auto">${esc(ps.note)}</span></div>` : '';
}

function poster(a, mine, inst, wide, w) {
  const st = status(a), job = S.jobs[a.id];
  const own = !S.library[a.id] && S.owned[a.id];
  const badge = S.missing && S.missing.has(a.id) ? `<span class="badge tr miss">${esc(t('Missing'))}</span>`
    : st === 'update' ? `<span class="badge tr">${esc(t('Update'))}</span>`
    : st === 'running' ? `<span class="badge tr">${esc(t('Playing'))}</span>`
      : st === 'soon' ? `<span class="badge tr soon"${notifyOn(a.id) ? ` title="${esc(t('We\'ll email you when it comes out.'))}"` : ''}>${notifyOn(a.id) ? BELL(12) : ''}${esc(t('Soon'))}</span>`
        : st === 'installed' && !mine ? `<span class="badge tl">${esc(t('Installed'))}</span>`
          : !S.library[a.id] && isSaved(a.id) && !mine ? `<span class="badge tl lib">${esc(t('In Library'))}</span>`
            : own ? `<span class="badge tl">${esc(t('You have it'))}</span>` : '';
  const lb = S.library[a.id] && S.library[a.id].local ? `<span class="badge tl dev">${esc(S.library[a.id].tag === 'own' ? t('Added by You') : t('Local build'))}</span>` : '';
  // Easy Setup on turnkey recomps. Setup Required only on a game in the player's Library, as a to-do.
  const easy = S.library[a.id] && needsSetup(a) && !a.soon && !setupDone(a.id) ? `<span class="badge easy setupreq">${WRENCH.replace(/18/g, '13')}${esc(t('Setup Required'))}</span>`
    : !S.library[a.id] && turnkey(a) ? `<span class="badge easy">${DISC_S}${esc(t('Easy Setup'))}</span>` : '';
  const prog = job ? `<span class="bar-p"><i data-prog-bar="${esc(a.id)}" style="width:${pct(job)}%"></i></span>` : '';
  const vg = !mine && !prog && verGroup(a);
  const vers = vg ? `<span class="badge vers" title="${esc(tn(vg.list.length, '1 version', '{n} versions'))}">${LAYERS}${fmtNum(vg.list.length)}</span>`
    : inst > 1 && !prog ? `<span class="badge tl" title="${esc(tn(inst, '1 installed', '{n} installed'))}">${LAYERS}${esc(tn(inst, '1 installed', '{n} installed'))}</span>` : '';
  // Great on Deck: bottom right, above the versions count when there is one.
  const deckb = greatOnDeck(a) && !prog ? `<span class="badge deck${vers ? ' up' : ''}" title="${esc(t('Great on Deck: Steam Deck players say it plays great'))}">${DECK_ICON(13)}${esc(t('Deck'))}</span>` : '';
  // Ended or on hold: a badge at the top left, under the one already there.
  const ps = projectState(a), tlUsed = !!lb || /badge tl/.test(badge) || /badge tl/.test(vers);
  const stb = ps && ps.kind !== 'quiet' ? `<span class="badge tl state ${ps.kind}${tlUsed ? ' low' : ''}">${esc(t(STATE_NAME[ps.kind]))}</span>` : '';
  if (wide) {
    // A wide tile: the game's art, blurred and darkened, with its logo in the middle (its name
    // when there's no logo).
    const art = a.hero || a.cover, logo = a.logo;
    return `<div class="poster wide${lb ? ' local' : ''}"><span class="ph"></span>${art ? `<img class="wbg" src="${esc(media(thumbOf(art)))}" alt="" loading="lazy" decoding="async">` : ''}<span class="wshade"></span>${logo ? `<img class="wlogo" src="${esc(media(logo))}" alt="${esc(a.name)}" loading="lazy" decoding="async">` : `<span class="wname">${esc(a.name)}</span>`}${lb || badge}${stb}${prog ? prog : easy}${vers}${deckb}</div>`;
  }
  return `<div class="poster${lb ? ' local' : ''}"><span class="ph">${esc(a.name)}</span>${pic(thumbOf(a.cover, Math.max(w || 0, tileThumb())), '', 'loading="lazy"')}${lb || badge}${stb}${prog ? prog : easy}${vers}${deckb}</div>`;
}

// The line under one game of a recomp's tile: its setup still to do, that it's picked in the
// recomp's own menu, or which recomp it's part of.
function gameTileLine(a) {
  const g = S.library[a.id];
  if (g && g.ready === false) return `<span class="tl-setup">${esc(t('Set Up'))}</span>`;
  if (a.menuPick) return `<span>${esc(t('Pick the game in its menu'))}</span>`;
  return `<span>${esc(tf('Part of {name}', { name: a.parentName || (byId(a.parent) || {}).name || '' }))}</span>`;
}
// On one game's page: the recomp it's part of, as a link.
function partOfLine(a) {
  if (!a.parent) return '';
  return `<div class="part-of"><button type="button" class="link-btn" data-act="open" data-id="${esc(a.parent)}">${esc(tf('Part of {name}', { name: a.parentName || (byId(a.parent) || {}).name || '' }))}</button>${a.menuPick ? `<span>${esc(t('Pick the game in its menu when it starts.'))}</span>` : ''}</div>`;
}
// On a recomp's page: the games it includes, each a tile. One row when they fit, else rows
// split evenly (never one left over on its own).
function includesBox(a) {
  if (a.parent) return '';
  const list = gamesOf(a.id, a).map((g) => byId(a.id + '--' + g.id)).filter(Boolean);
  if (!list.length) return '';
  const rows = Math.ceil(list.length / 6), cols = Math.ceil(list.length / rows);
  return `<section class="gp-includes"><h2>${esc(tn(list.length, 'Includes 1 Game', 'Includes {n} Games'))}</h2><div class="grid inc-grid" data-n="${list.length}" style="--inc-cols:${cols}">${list.map((x) => tile(x, { quick: true })).join('')}</div></section>`;
}
function tile(a, opts = {}) {
  // The glow behind a tile is blurred, so its smallest copy is plenty (the poster itself is the
  // one that has to be sharp, and can still load only when it scrolls in).
  const cov = a.cover ? media(thumbOf(a.cover, 160)) : '';
  const sub = opts.sub != null ? opts.sub : a.parent ? gameTileLine(a) : a.game ? `${DISC_S}<span>${esc(a.game)}</span>` : `<span>${esc(originName(a.origin))}</span>`;
  const vt = (devOf(a) || {}).verified;
  const quick = opts.quick && S.library[a.id] && !S.jobs[a.id];
  // Not in the Library yet: add it from the cover; added: install from the cover.
  const qadd = !quick && !opts.mine && !S.library[a.id] && !S.jobs[a.id] && status(a) === 'get';
  const qsaved = qadd && isSaved(a.id);
  return `<div class="tile${opts.on ? ' on' : ''}" data-id="${esc(a.id)}">${cov ? `<span class="glow" style="background-image:url('${cssUrl(cov)}')"></span>` : ''}
    <button type="button" class="tile-b" data-act="${opts.act || 'open'}" data-id="${esc(a.id)}" aria-label="${esc(a.name)}">${poster(a, opts.mine, opts.inst, opts.wide, opts.w)}${vt ? '<span class="trow">' : ''}<span class="t"${shortName(a.name) !== a.name ? ` data-short="${esc(shortName(a.name))}" data-full="${esc(a.name)}"` : ''} title="${esc(a.name)}">${esc(a.name)}</span>${vt ? `${TICK(15)}</span>` : ''}<span class="s">${sub}${opts.wide ? '' : osMarks(a)}</span></button>
    ${quick ? `<span class="qpw"><button type="button" class="qp" data-act="${S.running.has(a.id) ? 'resume' : 'play'}" data-id="${esc(a.id)}" aria-label="${esc(tf('Play {name}', { name: a.name }))}" title="${esc(t('Play'))}">${PLAY}</button></span>` : ''}${qadd ? `<span class="qpw"><button type="button" class="qp${qsaved ? '' : ' add'}" data-act="${qsaved ? 'install' : 'addlib'}" data-id="${esc(a.id)}" aria-label="${esc(qsaved ? tf('Install {name}', { name: a.name }) : tf('Add {name} to your Library', { name: a.name }))}" title="${esc(qsaved ? t('Install') : t('Add to Library'))}">${qsaved ? DOWN : PLUS}</button></span>` : ''}</div>`;
}

const pct = (j) => (j && j.total ? Math.min(100, Math.round((j.done / j.total) * 100)) : 0);
function progLabel(j) {
  if (!j) return '';
  if (j.phase === 'prepare') return [t('Getting ready'), ''];
  if (j.phase === 'queued') return [t('Waiting for the download before it'), ''];
  if (j.phase === 'paused') return [t('Paused'), j.total ? tf('{done} of {total}', { done: fmtB(j.done), total: fmtB(j.total) }) : ''];
  if (j.phase === 'backup') return [t('Backing up your saves'), ''];
  if (j.phase === 'verify') return [t('Checking the download'), ''];
  if (j.phase === 'unpack') return [t('Unpacking'), ''];
  if (j.phase === 'clone') return [t('Getting its project files'), ''];
  return [tf('{done} of {total}', { done: fmtB(j.done), total: fmtB(j.total) }), j.speed ? tf('{speed}/s', { speed: fmtB(j.speed) }) : ''];
}
function progress(id) {
  const j = S.jobs[id] || (S.paused || {})[id];
  const [l, r] = progLabel(j);
  const busy = !j || (j.phase !== 'download' && j.phase !== 'paused') || !j.total;
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
// On a Steam Deck its own buttons (L1/R1) unless another kind of pad is in use.
const padKind = () => { const k = S.padKind || ((S.pads || [])[0] || {}).kind || 'xbox'; return S.deck && k === 'xbox' ? 'steam' : k; };
const PAD_NIN = { a: 'A', b: 'B', x: 'X', y: 'Y', lb: 'L', rb: 'R', view: N_('Minus'), menu: N_('Plus') };
// The icon file and button names for a kind of controller: xbox, ps or nintendo.
const PAD_STEAM = { a: 'A', b: 'B', x: 'X', y: 'Y', lb: 'L1', rb: 'R1', view: N_('View'), menu: N_('Menu') };
window.padArt = (k, n) => { const ps = k === 'ps', nin = k === 'nintendo', st = k === 'steam'; return { url: `img/pad/${ps ? 'ps' : nin ? 'nintendo' : st ? 'steam' : 'xbox'}-${n}.${ps ? 'png' : 'svg'}`, name: (ps ? PAD_PS : nin ? PAD_NIN : st ? PAD_STEAM : PAD_NAME)[n] || n }; };
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

const PAUSE = '<svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><rect x="6" y="4.5" width="4" height="15" rx="1.2"/><rect x="14" y="4.5" width="4" height="15" rx="1.2"/></svg>';
const XS = '<svg width="14" height="14" viewBox="0 0 12 12" aria-hidden="true"><path d="m1.5 1.5 9 9m0-9-9 9" stroke="currentColor" stroke-width="1.6"/></svg>';
// Only a game's own download can pause (and carry on later): not unpacking, setting up or a plugin.
const canPause = (id) => { const j = S.jobs[id]; return !!j && !j.plug && (j.phase === 'download' || j.phase === 'queued'); };

// The main button for a game, whatever state it's in.
function actions(a, size = 'big') {
  const st = status(a), id = esc(a.id), sq = size === 'big' ? 'btn sq glass' : 'btn sq sm', more = `<button type="button" class="${sq}" data-act="menu" data-id="${id}" aria-label="${esc(t('More'))}" title="${esc(t('More'))}">${MORE}</button>`;
  const b = (cls, act, label, icon = '') => `<button type="button" class="btn ${size} ${cls}" data-act="${act}" data-id="${id}">${icon}${label}</button>`;
  switch (st) {
    case 'running': return b('main', 'resume', esc(t('Back to the game')), PLAY) + b('', 'stop', esc(t('Close game'))) + more;
    case 'settingup': return `<button type="button" class="btn ${size}" disabled>${esc(t('Its setup is open'))}</button>` + more;
    case 'busy': return progress(a.id) + (canPause(a.id) ? `<button type="button" class="${sq}" data-act="dl-pause" data-id="${id}" aria-label="${esc(t('Pause'))}" title="${esc(t('Pause'))}">${PAUSE}</button>` : '') + `<button type="button" class="${sq}" data-act="cancel" data-id="${id}" aria-label="${esc(t('Cancel'))}" title="${esc(t('Cancel'))}">${XS}</button>`;
    case 'paused': return progress(a.id) + `<button type="button" class="${sq}" data-act="dl-resume" data-id="${id}" aria-label="${esc(t('Resume'))}" title="${esc(t('Resume'))}">${PLAY.replace(/18/g, '14')}</button><button type="button" class="${sq}" data-act="cancel" data-id="${id}" aria-label="${esc(t('Cancel'))}" title="${esc(t('Cancel'))}">${XS}</button>` + (size === 'big' ? more : '');
    case 'update': return b('main', 'install', esc(t('Update')), DOWN) + b('', 'play', esc(t('Play')), PLAY) + more;
    case 'installed': return setupDue(a) ? b('main warn', 'setupreq', esc(t('Set Up')), WRENCH) + more : S.easy.has(a.id) && S.setup[a.id] && S.setup[a.id].needs && (!S.setup[a.id].copy || needUpdate(S.setup[a.id])) ? b('main', 'setup', esc(t('Set up')), DISC_S.replace(/14/g, '18')) + more : isServer(a) ? b('main', 'play', esc(t('Start Server')), PLAY) + more : size === 'big' && otherInstalled(a).length ? `<span class="split">${b('main', 'play', esc(t('Play')), PLAY)}<button type="button" class="btn ${size} main caret" data-act="play-pick" data-id="${id}" aria-label="${esc(t('Play another version'))}" title="${esc(t('Play another version'))}"><svg width="14" height="14" viewBox="0 0 12 12" aria-hidden="true"><path d="m2.5 4.5 3.5 3.5 3.5-3.5" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/></svg></button></span>` + more : b('main', 'play', esc(t('Play')), PLAY) + more;
    case 'soon': return (a.custom ? `<button type="button" class="btn ${size}" disabled>${esc(t('Coming soon'))}</button>` : notifyOn(a.id) ? b('notify-on', 'notify', esc(t('We\'ll Email You')), BELL(18)) : b('main', 'notify', esc(t('Notify Me')), BELL(18))) + more;
    case 'nobuild': return S.os === 'linux' && hasWindowsBuild(a) ? b('main', 'use-proton', esc(t('Play With Proton')), PLAY) + more : `<button type="button" class="btn ${size}" disabled>${esc(tf('No {os} version yet', { os: OS_NAME[S.os] }))}</button>` + more;
    // Easy Setup games say so: one press installs it and asks for the player's copy.
    default: return (isSaved(a.id) ? (turnkey(a) ? b('main easy', 'install', esc(t('Easy Install')) + (a.size ? ` <span style="opacity:.7;font-weight:600">· ${esc(a.size)}</span>` : ''), DISC_S.replace(/14/g, '18')) : b('main', 'install', esc(t('Install')) + (a.size ? ` <span style="opacity:.7;font-weight:600">· ${esc(a.size)}</span>` : ''), DOWN)) : b('main', 'addlib', esc(t('Add to Library')), PLUS)) + (needsSetup(a) && size === 'big' ? b('warn-o', 'setupreq', esc(t('Setup Required')), WRENCH) : '') + (size === 'big' && !a.custom ? b('glass', 'have-it', esc(t('I Already Have It'))) : '') + more;
  }
}

// A game's quick actions, at the top of its side column: Add to Steam and to the desktop
// (installed games), its project page and Report a problem (games with a repo). Each has its icon
// and its name. Steam's own icon is used when ui/icons/steam.svg is there.
const QI = (d, w = 1.9) => `<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="${w}" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${d}</svg>`;
const QI_PAD = QI('<path d="M7.5 7.5h9a4.5 4.5 0 0 1 4.4 5.4l-.8 3.9a2.3 2.3 0 0 1-3.9 1.1L14 15.5h-4l-2.2 2.4a2.3 2.3 0 0 1-3.9-1.1l-.8-3.9a4.5 4.5 0 0 1 4.4-5.4z"/><path d="M8 10.5v3M6.5 12h3"/><circle cx="15.5" cy="11" r=".9" fill="currentColor" stroke="none"/><circle cx="17.3" cy="13" r=".9" fill="currentColor" stroke="none"/>');
const QI_DESK = QI('<rect x="3" y="4" width="18" height="12.5" rx="2"/><path d="M9 20.5h6M12 16.5v4"/>');
const QI_REPORT = QI('<path d="M10.3 4.1 2.6 17.5A2 2 0 0 0 4.3 20.5h15.4a2 2 0 0 0 1.7-3L13.7 4.1a2 2 0 0 0-3.4 0z"/><path d="M12 9.5v4.5M12 17h.01"/>', 2);
let STEAM_ICON = '';
fetch('icons/steam.svg').then((r) => (r.ok ? r.text() : '')).then((t) => { if (/^\s*<svg[\s>]/i.test(t) && !/<script|on\w+=/i.test(t)) { STEAM_ICON = t.replace(/<svg/i, '<svg class="qi-brand"'); if (!/aria-hidden/.test(STEAM_ICON)) STEAM_ICON = STEAM_ICON.replace(/<svg/i, '<svg aria-hidden="true"'); if (typeof S !== 'undefined' && S.view === 'game' && typeof refreshGame === 'function') refreshGame(); } }).catch(() => {});
const SUPPORT_I = '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="4" y="3" width="16" height="18" rx="3"/><path d="m8 9 1.5 1.5L12 8M8 15l1.5 1.5L12 14M14.5 9.5H17M14.5 15.5H17"/></svg>';
// A new issue on the recomp's own page, asking for RecompLauncher support. Opened for the player to
// send themselves; nothing is sent from here.
function askSupportUrl(a) {
  const title = 'RecompLauncher support';
  const body = 'Hi! Could you add a recomplauncher.json to this recomp, so RecompLauncher can set it up and show its settings for players?\n\nThe guide: https://recomplauncher.com/sdk-files/guide.md';
  if (isSF(a.repo)) return repoIssues(a.repo);
  if (isLab(a.repo)) return `${repoHome(a.repo)}/-/issues/new?issue[title]=${encodeURIComponent(title)}&issue[description]=${encodeURIComponent(body)}`;
  return `https://github.com/${a.repo}/issues/new?title=${encodeURIComponent(title)}&body=${encodeURIComponent(body)}`;
}
// What RecompLauncher can do for this recomp, and who made each part work: the catalog's
// "support", the recomp's own recomplauncher.json in its folder, or (offline) its boot profile.
async function supportModal(id) {
  const a = byId(id) || (S.library[id] && libApp(S.library[id]));
  if (!a) return;
  const sup = a.support && typeof a.support === 'object' ? a.support : null;
  let local = null;
  if (S.library[id]) local = await invoke('support_local', { id }).catch(() => null);
  const boot = a.boot || builtinBoot()[id] || {};
  const files = boot.files && typeof boot.files === 'object' ? boot.files : null;
  const guess = {
    easy_setup: !!(files && TURNKEY_HOW.includes(files.how)) || S.easy.has(id),
    settings: !!(boot.settings && (boot.settings.settings || []).length),
    prepare: !!(boot.setup || boot.prepare),
    verified: !!((Array.isArray(a.copy_ids) ? a.copy_ids.length : a.copy_ids) || (files && files.sha256)),
    saves: false,
  };
  // Capability sources: 'dev' means a manifest, not confirmed maintenance by the developer.
  // 'launcher', '' (not there), or '?' (there, source unknown) keep their catalog meanings.
  const who = (k) => (local && local[k] ? 'dev' : sup ? (sup[k] === 'dev' || sup[k] === 'launcher' ? sup[k] : '') : guess[k] ? '?' : '');
  const sdk = local ? 'release' : sup && sup.sdk ? sup.sdk : 'unknown';
  const hh = a.handheld && a.handheld.ok;
  const saves = who('saves') === 'dev' ? 'dev' : 'launcher';
  const rows = [
    [t('Easy Setup'), t('Pick your copy and the launcher sets it up.'), who('easy_setup')],
    [t('Game Settings in the Launcher'), t('Change the game\'s own settings from its page.'), who('settings')],
    [t('Full Setup Step'), t('The recomp\'s own setup step, run for you.'), who('prepare')],
    [t('Verified Copies Badge'), t('Shows when your copy matches a known good one.'), who('verified')],
    [t('Save Backups and Cloud Saves'), saves === 'dev' ? t('The recomp tells the launcher exactly where it saves.') : t('Always on from the launcher\'s side.'), saves],
    [t('Handheld Rating'), hh ? tf('Handheld players say it runs well: {devices}', { devices: (Array.isArray(a.handheld.devices) ? a.handheld.devices : []).filter((d) => typeof d === 'string').join(', ') || t('several players') }) : t('From handheld players\' reports. None yet.'), hh ? 'players' : ''],
  ];
  const tag = (w) => (w === 'dev' ? `<span class="tag ok">${esc(t('From Manifest'))}</span>` : w === 'launcher' ? `<span class="tag cy">${esc(t('By RecompLauncher'))}</span>` : w === 'players' ? `<span class="tag">${esc(t('By Players'))}</span>` : '');
  // What the developer's own file could add (the handheld rating comes from players).
  const missing = rows.slice(0, 4).some((r) => !r[2]);
  const source = local ? t('Manifest found in the installed files.') : sdk === 'release' ? t('Manifest included with the release.') : sdk === 'repo' ? t('Manifest in the project repository.') : '';
  // Only an explicit catalog declaration confirms who maintains the integration. A working
  // manifest may have been supplied by the player, launcher, or another contributor.
  const owner = a.maintained_by === 'official' ? 'project' : ['project', 'launcher', 'community'].includes(a.maintained_by) ? a.maintained_by : 'unconfirmed';
  const ask = a.repo && (sdk === 'none' || missing);
  const html = `<h2>${esc(t('Launcher Support'))}</h2><p class="muted">${esc(tf('What RecompLauncher can do for {name}, and who made it work.', { name: a.name }))}</p>
    <div class="mh sup-list">${rows.map(([k, d, w]) => `<div class="mh-row sup-row"><span class="sup-mark">${okMark(w ? true : null)}</span><div class="mh-k"><b>${esc(k)}</b><span class="sup-d">${esc(d)}</span></div>${tag(w)}</div>`).join('')}</div>
    ${source ? `<p class="muted">${esc(source)}</p>` : ''}
    <p class="sup-own">${esc(t(OWNER_LONG[owner]))}</p>
    ${ask ? `<p>${esc(t('Is a feature missing? Ask the developer to add RecompLauncher support.'))}</p>` : ''}
    <div class="acts ${ask ? 'grid2' : ''}">${ask ? `<button type="button" class="btn" data-x="ask">${esc(t('Ask the Developer'))}</button>` : ''}<button type="button" class="btn main" data-x="no">${esc(t('Close'))}</button></div>`;
  modal(html, (x) => { if (x.dataset.x === 'ask') invoke('open_url', { url: askSupportUrl(a) }).catch(fail); }, t('Launcher Support'));
}

function quickActs(a) {
  const id = esc(a.id), b = (act, label, icon, extra = '') => `<button type="button" class="btn" data-act="${act}" data-id="${id}" ${extra}>${icon}<span>${esc(label)}</span></button>`;
  const out = [];
  if (S.library[a.id] && S.os !== 'macos') {
    if (S.steam) out.push(b('steam', t('Add to Steam'), STEAM_ICON || QI_PAD));
    out.push(b('desktop', t('Add to desktop'), QI_DESK));
  }
  if (followLink(a)) {
    const f = followLink(a);
    out.push(b('url', followLabel(f), onX(f) ? X_S : BRANCH_S.replace('width="15" height="15"', 'width="18" height="18"'), `data-url="${esc(f)}"`));
  }
  if (a.repo) {
    out.push(b('url', isSF(a.repo) ? t('View on SourceForge') : hostName(a.repo), isLab(a.repo) || isSF(a.repo) ? BRANCH_S.replace('width="15" height="15"', 'width="18" height="18"') : GITHUB_S.replace('width="15" height="15"', 'width="18" height="18"'), `data-url="${esc(repoHome(a.repo))}"`));
    out.push(b('url', t('Report a problem'), QI_REPORT, `data-url="${esc(reportUrl(a))}"`));
  }
  if (!a.soon) out.push(b('support', t('Launcher Support'), SUPPORT_I));
  if (!out.length) return '';
  // Even rows: an odd one out takes the whole row.
  if (out.length % 2) out[out.length - 1] = out[out.length - 1].replace('class="btn"', 'class="btn span2"');
  return `<div class="gp-quick acts grid2">${out.join('')}</div>`;
}

// The other versions of this game that are installed too.
const otherInstalled = (a) => { const g = a && a.title && index().titles.get(a.title); return g ? g.every.filter((v) => v.id !== a.id && S.library[v.id]) : []; };
// A checkmark, a warning, or (null) a grey "don't know".
const okMark = (v) => (v ? `<span class="mark ok" title="${esc(t('Yes'))}"><svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round" aria-label="${esc(t('Yes'))}"><path d="m5 12.5 4.5 4.5L19 7.5"/></svg></span>`
  : v === null ? `<span class="mark maybe" title="${esc(t('Not sure'))}"><svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" aria-label="${esc(t('Not sure'))}"><path d="M7 12h10"/></svg></span>`
    : `<span class="mark no" title="${esc(t('Missing'))}"><svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" aria-label="${esc(t('Missing'))}"><path d="M12 7v6M12 17h.01"/></svg></span>`);

// Before playing: is everything this recomp needs here? Your copy of the game, the graphics
// driver, the processor, the Windows runtime, and room on the drive.
// The found copy isn't the right one: the player picks theirs.
const fixBtn = (id) => `<button type="button" class="btn sm" data-act="found-fix" data-id="${esc(id)}">${esc(t('Not This One'))}</button>`;
// Everything found for a game (its discs, update and extra content), loaded once per scan.
async function loadCopies(id) {
  S.copyFiles = S.copyFiles || {};
  if (S.copyFiles[id]) return S.copyFiles[id];
  const own = S.owned[id], drop = (((S.settings.ui || {}).found_drop) || {})[id] || [];
  try { S.copyFiles[id] = await invoke('copy_files', { id, picked: own ? own.path : null, dropped: drop }); } catch (e) { S.copyFiles[id] = { files: own ? [{ path: own.path, name: baseName(own.path), folder: '', size: own.size || 0, region: own.region || '', kind: 'game', disc: null, verified: false, picked: true }] : [], missing_discs: [] }; }
  return S.copyFiles[id];
}
// "Game detected · 3 files found", opening the list of them.
function copyLine(id) {
  const c = (S.copyFiles || {})[id];
  if (!c && !(S.copyBusy || {})[id]) {
    S.copyBusy = Object.assign(S.copyBusy || {}, { [id]: true });
    loadCopies(id).then(() => { delete S.copyBusy[id]; if (S.view === 'game' && S.arg === id) refreshGame(); });
  }
  const n = c ? Math.max(c.files.length, 1) : 1;
  const gap = c && c.missing_discs.length ? ' · ' + tn(c.missing_discs.length, 'Disc {list} is missing', 'Discs {list} are missing').replace('{list}', c.missing_discs.join(', ')) : '';
  return `<button type="button" class="linkish copy-line" data-act="copies" data-id="${esc(id)}">${esc(t('Game detected'))} · ${esc(tn(n, '1 file found', '{n} files found'))}${esc(gap)}</button>`;
}
const pickBtn = (id) => `<button type="button" class="btn sm" data-act="found-fix" data-id="${esc(id)}">${esc(t('Choose Your Copy'))}</button>`;
function readyRows(a) {
  const id = a.id, g = S.library[id], pc = S.pc || {}, rows = [];
  const own = S.owned[id], su = S.setup[id];
  if (su && su.needs) rows.push({ k: t('Your copy of the game'), v: su.copy ? true : own ? null : null,
    d: su.copy ? tf('Set up with {file}', { file: baseName(su.copy) }) : own ? '' : t('Insert it once and the launcher sets the game up for you.'), html: !su.copy && own ? copyLine(id) : '',
    tags: su.copy || own ? '' : fmtTags(copyFormats(id).exts),
    fix: su.copy ? '' : `<button type="button" class="btn sm main" data-act="setup" data-id="${esc(id)}">${esc(own ? t('Set it up') : t('Insert it'))}</button>` });
  else if (!g && S.easy.has(id)) rows.push({ k: t('Your copy of the game'), v: own ? true : null,
    d: own ? t('It\'s set up for you after it installs.') : t('You insert it after it installs, and the launcher sets the game up.'), html: own ? copyLine(id) : '', tags: own ? '' : fmtTags(copyFormats(id).exts), fix: '' });
  else if (a.needs) rows.push({ k: t('Your copy of the game'), v: own ? true : g && g.last_played ? true : null,
    d: own ? '' : g && g.last_played ? t('Set up the first time you played') : t('Not found'), html: own ? copyLine(id) : '',
    fix: own || (g && g.last_played) ? '' : pickBtn(id) });
  if (su && su.needs && su.update) rows.push({ k: t('The game\'s update'), v: su.update_copy ? true : su.update_optional ? true : null,
    d: su.update_copy ? tf('Set up with {file}', { file: baseName(su.update_copy) }) : su.update,
    fix: su.update_copy || su.update_optional ? '' : `<button type="button" class="btn sm main" data-act="setupdate" data-id="${esc(id)}">${esc(t('Choose It'))}</button>` });
  if (pc.vulkan != null) rows.push({ k: t('Graphics (Vulkan)'), v: pc.vulkan, d: pc.vulkan ? t('Your graphics driver has it') : t('Update your graphics driver to get it'), fix: pc.vulkan ? '' : `<button type="button" class="btn sm" data-act="url" data-url="https://www.vulkan.org/tools#download-these-essential-development-tools">${esc(t('How'))}</button>` });
  if (S.os !== 'macos' && (a.origin === 'x360' || (a.requires && a.requires.avx2))) rows.push({ k: t('Processor (AVX2)'), v: !!pc.avx2, d: pc.avx2 ? t('Your processor has it') : a.origin === 'x360' ? t('Xbox 360 recomps need a processor from 2013 or later') : t('It needs a processor from 2013 or later'), fix: '' });
  // A recomp whose own setup builds it on this PC (from a clone of its repo).
  const builds = !!((a.boot && a.boot.clone) || (builtinBoot()[id] || {}).clone);
  // What a setup that builds or runs scripts says it needs ("needs": ["build_tools", "clang", "python"]
  // in its profile, or the same as "requires" flags).
  const bootOf = a.boot || builtinBoot()[id] || {};
  const needs = new Set([...(Array.isArray(bootOf.needs) ? bootOf.needs : []), ...Object.keys(a.requires || {}).filter((k) => a.requires[k] === true)]);
  const builds2 = builds || needs.has('build_tools') || needs.has('clang');
  if (!builds && builds2 && pc.build_tools != null) rows.push({ k: t('C++ build tools'), v: pc.build_tools, d: pc.build_tools ? t('Installed') : t('Its setup builds the game on your PC with Visual Studio\'s free C++ build tools'), fix: pc.build_tools ? '' : `<button type="button" class="btn sm main" data-act="url" data-url="https://visualstudio.microsoft.com/visual-cpp-build-tools/">${esc(t('Get them'))}</button>` });
  if (needs.has('clang') && pc.clang != null) rows.push({ k: t('Clang'), v: pc.clang, d: pc.clang ? t('Installed') : t('In the Visual Studio Installer, tick "C++ Clang tools for Windows" under Individual components'), fix: pc.clang ? '' : `<button type="button" class="btn sm main" data-act="url" data-url="https://learn.microsoft.com/cpp/build/clang-support-msbuild">${esc(t('How'))}</button>` });
  if (needs.has('python') && pc.python != null) { const v = pc.python || '', ok = /^3\.(1\d|[2-9]\d)/.test(v); rows.push({ k: t('Python 3.10 or newer'), v: ok, d: ok ? tf('Python {version}', { version: v }) : v ? tf('Python {version} is too old', { version: v }) : t('Its setup runs Python scripts'), fix: ok ? '' : `<button type="button" class="btn sm main" data-act="url" data-url="https://www.python.org/downloads/windows/">${esc(t('Get it'))}</button>` }); }
  if (builds && pc.build_tools != null) rows.push({ k: t('C++ build tools'), v: pc.build_tools, d: pc.build_tools ? t('Installed') : t('Its setup builds the game on your PC with Visual Studio\'s free C++ build tools'), fix: pc.build_tools ? '' : `<button type="button" class="btn sm main" data-act="url" data-url="https://visualstudio.microsoft.com/visual-cpp-build-tools/">${esc(t('Get them'))}</button>` });
  if (pc.vcredist != null) { const old = pc.vcredist && pc.vcredist_old; rows.push({ k: t('Visual C++ runtime'), v: pc.vcredist && !old, d: old ? tf('Out of date ({version}). Newer recomps need a newer one.', { version: pc.vcredist_old }) : pc.vcredist ? t('Installed') : t('Almost every Windows recomp needs it'), fix: pc.vcredist && !old ? '' : `<button type="button" class="btn sm main" data-act="vcredist">${esc(old ? t('Update It') : t('Get it'))}</button>` }); }
  if (!g && a.size) {
    const lib = (S.libs || []).find((l) => l.default);
    const need = (a.requires && Number(a.requires.disk_gb) > 0 ? Number(a.requires.disk_gb) * 1073741824 : 0) || parseSize(a.size) * 3;
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
    ${rows.map((r) => `<div class="set-row"><div class="k"><b>${esc(r.k)}</b>${r.html ? `<span>${r.html}${r.d ? ' ' + esc(r.d) : ''}</span>` : `<span>${esc(r.d)}</span>`}${r.tags ? `<span class="fmts">${r.tags}</span>` : ''}</div><div class="acts">${r.fix}${r.fix && r.v !== true ? '' : okMark(r.v)}</div></div>`).join('')}</div>`;
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
// The player's own words about a game (Customize, Info): on its page, above everything about it.
const ownAbout = (id) => { const ab = (S.settings.ui || {}).abouts || {}; return String(ab[id] || ab[gameKey(byId(id))] || '').trim(); };
function ownAboutCard(id) {
  const txt = ownAbout(id);
  if (!txt) return '';
  return `<div class="card own-about"><div class="card-h" style="margin-bottom:10px"><h3>${esc(t('Your Description'))}</h3><button type="button" class="btn sm" data-act="customize" data-id="${esc(id)}">${esc(t('Edit'))}</button></div>${txt.split(/\n\s*\n/).map((p) => `<p dir="auto">${esc(p.trim())}</p>`).join('')}</div>`;
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

// The facts as small tiles, the label over its value, in even rows: three across when they fill
// rows of three, otherwise two, and a last odd one takes the whole row.
function factGrid(facts) {
  const cols = facts.length % 3 === 0 ? 3 : 2;
  const odd = cols === 2 && facts.length % 2 === 1;
  return `<dl class="facts-grid c${cols}">${facts.map(([k, v], i) => `<div${odd && i === facts.length - 1 ? ' class="wide"' : ''}><dt>${esc(k)}</dt><dd dir="auto">${esc(v)}</dd></div>`).join('')}</dl>`;
}
// About the original game, from IGDB: what it is, then the facts there are.
function igdbCard(ab) {
  const facts = aboutFacts(ab), score = Number.isFinite(Number(ab.rating)) && ab.rating !== null && ab.rating !== '' ? Math.round(Number(ab.rating)) : null;
  return `<div class="card about-game igdb"><div class="card-h" style="margin-bottom:10px"><h3>${esc(t('About the game'))}</h3>${score !== null ? `<span class="score-b" title="${esc(t('Critic and player score'))}"><b>${score}</b>${esc(t('Critic and player score'))}</span>` : ''}</div>
    ${ab.summary ? String(ab.summary).split(/\n\s*\n/).map((p) => `<p dir="auto">${esc(p.trim())}</p>`).join('') : ''}
    ${facts.length ? factGrid(facts) : ''}
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
      ${a.cover ? pic(thumbOf(a.cover, 160), '', 'loading="lazy"') : '<span class="ph"></span>'}
      <span class="in"><span class="kick">${esc(x.name)}</span><b>${esc(x.title)}</b><small>${esc(date(x.date))}${x.have ? ' · ' + esc(t('Installed')) : S.updates[x.id] === x.tag ? ' · ' + esc(t('Ready to update')) : ''}</small><span class="p">${esc(notesLine(x.notes) || t('No notes for this one.'))}</span></span></button>`; }).join('')}</div></section>`;
}

// The "Offline" mark: the list is the copy on disk because the site couldn't be reached, or the
// site is resting after trouble (the launcher waits a while before asking it again).
const offNow = () => !!(S.offline || S.netOff);
// The site resting after trouble: "Offline", or, when it answered that it's busy (5xx, 429), that
// it's busy and the launcher is retrying.
function netPill() { const o = $('#offline'); if (!o) return; o.hidden = !offNow(); o.textContent = S.netBusy && !S.offline ? t('Website Busy, Retrying') : t('Offline'); }
T.event.listen('net', ({ payload }) => { S.netOff = !!(payload && payload.offline); S.netBusy = !!(payload && payload.busy); netPill(); });
invoke('net_state').then((n) => { S.netOff = !!(n && n.offline); S.netBusy = !!(n && n.busy); netPill(); }).catch(() => {});

// ---------- pages ----------
function offlineNote() {
  return S.offline ? `<div class="offline"><span style="width:8px;height:8px;border-radius:50%;background:var(--warn);flex:none"></span>${esc(t('You\'re offline, so this is the list as it was the last time the launcher could reach it.'))}</div>` : '';
}

function pageHome() {
  const apps = visible();
  if (!apps.length) return S.cat ? `<div class="page">${offlineNote()}<div class="empty"><h2>${esc(t('Nothing to discover right now'))}</h2></div></div>` : `<div class="loading">${esc(t('Loading…'))}</div>`;
  const mine = apps.filter(forOS);
  const pool = (mine.length ? mine : apps).filter((a) => !a.soon);
  const feats = turnkeyFirst(bannerApps() || oneEach(uniq([...pool.filter((a) => a.featured && (a.hero || a.cover)), ...pool.filter((a) => a.hero && notPC(a)).sort(byDl), ...pool.filter(notPC).sort(byDl)].map((a) => a.id)).map(byId)).slice(0, 5));
  S.slide = Math.min(S.slide || 0, Math.max(0, feats.length - 1));
  const ready = oneEach(pool.filter(turnkey).sort(byNew), true);
  const played = libRecs().sort((x, y) => (y.last_played || 0) - (x.last_played || 0) || (y.installed_at || 0) - (x.installed_at || 0)).map((g) => libApp(g));
  const found = oneEach(Object.keys(S.owned).filter((id) => !S.library[id]).map(byId).filter((a) => a && !a.soon && forOS(a) && !(S.settings.hide_ai && isAI(a)) && !isHidden(a.id) && !haveAny(a)).sort(byDlConsolesFirst));
  const easy = oneEach(pool.filter((a) => S.easy.has(a.id) && !haveAny(a)));
  const fav = favs().map((id) => libApp(S.library[id]));
  return `<div class="page home">${offlineNote()}
    ${feats.length ? slider(feats) : ''}
    ${played.length ? shelf('played', t('Jump back in'), played, { mine: true, quick: true, see: 'library', sub: (a) => esc(S.library[a.id] && S.library[a.id].last_played ? tf('Played {when}', { when: ago(S.library[a.id].last_played) }) : t('Not played yet')) }) : ''}
    ${fav.length ? shelf('favs', t('Favorites'), fav, { mine: true, quick: true }) : ''}
    ${ready.length >= 4 ? shelf('ready', t('Ready to Play'), ready, { note: t('The launcher sets these up for you. No extra steps.') }) : ''}
    ${shelf('updated', t('Recently updated'), recentlyUpdated(pool), { sub: (a) => esc(tf('Updated {when}', { when: ago(Date.parse(a.released) / 1000) })) })}
    ${follows().length ? shelf('follow', t('From developers you follow'), oneEach(pool.filter((a) => a.dev && follows().includes(a.dev)).sort(byNew), true), { sub: (a) => esc((devOf(a) || {}).name || a.author || '') }) : ''}
    ${found.length ? foundSection(found) : (S.settings.rom_dirs || []).length ? '' : foundCallout()}
    ${shelf('popular', t('Popular right now'), oneEach(pool.filter(notPC).sort(byDl)), { see: 'popular', turnkey: true })}
    ${shelf('new', t('Just added'), oneEach([...pool].sort(byNew), true), { see: 'new', turnkey: true })}
    ${easy.length >= 2 ? shelf('easy', t('Drop in Your Copy'), easy, { note: t('Pick your copy of the game (a disc image or ROM) and these are set up for you, with their settings right in the launcher.') }) : ''}
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
  const one = t('You already have this game. Install one and the launcher sets it up with your copy.');
  const many = t('You already have these games. Install one and the launcher sets it up with your copy.');
  // The ones already in the Library aren't here, so the count and the "more in Discover" note
  // would otherwise read as everything the player owns.
  const notYet = t('None of them are in your Library yet.');
  const intro = found.length === 1 ? one : many + ' ' + notYet;
  return `<section class="shelf"><div class="shelf-h"><div><h2>${esc(t('Found in your folders'))}</h2><p>${esc(intro)}${more ? ' ' + esc(tn(more, '1 more in Discover.', '{n} more in Discover.')) : ''}</p></div></div>
    <div class="news-grid${list.length === 1 ? ' one' : ''}">${list.map((a) => `<div class="news found-card"><button type="button" class="fc-open" data-act="open" data-id="${esc(a.id)}" aria-label="${esc(a.name)}">${a.cover ? pic(thumbOf(a.cover, 160), '', 'loading="lazy"') : '<span class="ph"></span>'}
      <span class="in"><span class="kick">${esc(t('You have it'))}</span><b>${esc(a.name)}</b><small class="based">${DISC_S}${esc(baseName(S.owned[a.id].path))}</small><span class="p">${esc([a.game, originName(a.origin)].filter(Boolean).join(' · '))}</span></span></button>
      <div class="acts">${S.jobs[a.id] ? progress(a.id) : `<button type="button" class="btn main" data-act="install" data-id="${esc(a.id)}">${DOWN}${esc(t('Install'))}</button>`}</div></div>`).join('')}</div></section>`;
}
// The games Ruben picks for the top of Home, in his order: this system's own list when the catalog
// has them per system ("banners": {windows, macos, linux}), else the one "banner" for all. Null when
// none of them can be shown here, and Home picks them itself as before. Big Picture's dock uses it too.
function bannerIds(cat, os) {
  const per = cat && cat.banners && typeof cat.banners === 'object' && Array.isArray(cat.banners[os]) ? cat.banners[os] : null;
  return per && per.length ? per : cat && Array.isArray(cat.banner) ? cat.banner : [];
}
window.bannerApps = () => {
  const ids = bannerIds(S.cat, S.os);
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
      <div class="sl-cover">${pic(thumbOf(a.cover))}</div></div>`).join('')}
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
// The hero only moves while the window is really in front. A merely unfocused or covered window
// (another app on top, the launcher behind a game) leaves it still, so an idle launcher draws no
// frames. Minimized/hidden is already handled by S.away and document.hidden.
const windowInFront = () => !document.hidden && document.hasFocus();
const markWindowFocus = () => document.body.classList.toggle('unfocused', !windowInFront());
addEventListener('focus', markWindowFocus);
addEventListener('blur', markWindowFocus);
document.addEventListener('visibilitychange', markWindowFocus);
markWindowFocus();
setInterval(() => {
  const s = $('#slider');
  if (!s || !windowInFront() || S.away || s.matches(':hover') || document.body.classList.contains('tv-on') || $('.scrim')) return;
  if (document.body.classList.contains('lighter') || S.running.size) return;
  showSlide((S.slide || 0) + 1);
}, 8000);
function shelf(key, title, list, o = {}) {
  if (!list.length) return '';
  return `<section class="shelf" data-shelf="${key}" data-n="${Math.min(24, list.length)}"><div class="shelf-h"><div><h2>${esc(title)}</h2>${o.note ? `<p>${esc(o.note)}</p>` : ''}</div>
    <div class="pager"><small></small>${o.see ? `<button type="button" class="see" ${o.see === 'library' ? 'data-go="library"' : `data-act="see-all" data-sort="${o.see}"`}>${esc(t('See all'))}</button>` : ''}<button type="button" class="ar" data-act="shelf" data-d="-1" aria-label="${esc(t('Back'))}">${CHEV_L}</button><button type="button" class="ar" data-act="shelf" data-d="1" aria-label="${esc(t('More'))}">${CHEV_R}</button></div></div>
    <div class="viewport"><div class="track">${(o.turnkey ? turnkeyFirst(list.slice(0, 24)) : list.slice(0, 24)).map((a) => tile(a, { mine: o.mine, quick: o.quick, sub: o.sub ? o.sub(a) : undefined })).join('')}</div></div></section>`;
}
// Each shelf shows as many games as fit, and pages through the rest.
function layoutShelves(scope = document) {
  // Measure every shelf before writing any of them: changing --per lays the page out again,
  // and reading the next shelf after that would make it lay out once per shelf.
  const rows = [];
  for (const sh of $$('.shelf[data-shelf]', scope)) {
    const vp = $('.viewport', sh), tr = $('.track', sh);
    if (!vp || !tr) continue;
    const w = tr.clientWidth;
    if (w) rows.push([sh, tr, w, Number(sh.dataset.n) || 0]);
  }
  const gap = 20;
  for (const [sh, tr, w, n] of rows) {
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

// With `ended`, the ended ones the filters would otherwise show (what "Hide ended" took out).
function discoverList(ended = false) {
  const f = S.find, q = fold(S.q.trim());
  let list = visible().filter((a) => (!f.genre || a.genre === f.genre) && (!f.origin || a.origin === f.origin) && (!f.tag || tagsOf(a).includes(f.tag)) && showOk(a, f.show) && forOS(a) && runsFits(a, runsPick()) && (!f.great || playsGreat(a)) && (!f.deckgreat || !onHandheld() || greatOnDeck(a)) && (!f.easy || turnkey(a)) && (!f.mp || isMulti(a)) && (ended ? isEnded(a) : keepEnded(a, f))
    && (!q || fold([a.name, a.game, a.author, originName(a.origin), GENRES[a.genre], t(GENRES[a.genre]), ...tagsOf(a)].join(' ')).includes(q)));
  const soonLast = (x, y) => (x.soon ? 1 : 0) - (y.soon ? 1 : 0);
  if (f.sort === 'new') list.sort(byNew); else if (f.sort === 'updated') list.sort(byUpdated); else if (f.sort === 'az') list.sort((x, y) => x.name.localeCompare(y.name)); else if (f.sort === 'stars') list.sort((x, y) => (y.stars || 0) - (x.stars || 0)); else list.sort(byDlConsolesFirst);
  if (!ended) S.findCount = recompCount(list);
  let out = oneEach(list.sort(soonLast), f.sort === 'new');
  // Discover's own order (Popular, not picked by the player): turnkey recomps first.
  if (f.sort === 'popular' && !f.picked) out = turnkeyFirst(out);
  // A search also finds the games inside recomps that carry several, after the recomps themselves.
  if (q && !ended) {
    const kids = [];
    for (const a of visible()) {
      if (!forOS(a) || !keepEnded(a, f)) continue;
      for (const g of gamesOf(a.id, a)) { const c = byId(a.id + '--' + g.id); if (c && !isHidden(c.id) && fold(c.name).includes(q)) kids.push(c); }
    }
    out = out.concat(kids);
  }
  return out;
}

const runsSel = () => `<label class="pill-sel">${esc(t('Runs On'))}<select class="sel" data-act="runs-sel">${runsList().map(([k, n]) => `<option value="${k}"${runsPick() === k ? ' selected' : ''}>${esc(t(n))}</option>`).join('')}</select></label>`;
// Discover's controls: on the page, and again in the bar pinned to the top once those scroll away.
const sortOpts = () => [['popular', t('Popular')], ['new', t('Newest')], ['updated', t('Recently updated')], ['stars', t('Most stars')], ['az', t('A to Z')]];
const pillSel = (label, act, opts, cur, cls = '') => `<label class="pill-sel${cls ? ' ' + cls : ''}">${esc(label)}<select class="sel" data-act="${act}">${opts.map(([k, n]) => `<option value="${esc(k)}"${cur === k ? ' selected' : ''}>${esc(n)}</option>`).join('')}</select></label>`;
const originSel = (f, cls) => pillSel(t('Console'), 'origin', [['', t('Any')], ...(S.cat.origins || []).map((o) => [o.id, o.name])], f.origin, cls);
const genreSel = (f, genres, cls) => pillSel(t('Genre'), 'genre-sel', [['', t('Any')], ...genres.map((k) => [k, t(GENRES[k])])], f.genre, cls);
const showSel = (f) => pillSel(t('Show'), 'show-sel', SHOWS.map(([k, n]) => [k, t(n)]), f.show || '');
const tagSel = (f) => pillSel(t('Tag'), 'tag-sel', [['', t('Any')], ...allTags().map((k) => [k, k])], f.tag || '');
const discToggles = (f) => `<button type="button" class="toggle" role="switch" aria-checked="${!!f.easy}" data-act="easy-f">${DISC_S}${esc(t('Easy Setup'))}<span class="switch" aria-hidden="true"></span></button>
      <button type="button" class="toggle" role="switch" aria-checked="${!!f.mp}" data-act="mp-f">${esc(t('Multiplayer'))}<span class="switch" aria-hidden="true"></span></button>
      <button type="button" class="toggle" role="switch" aria-checked="${!!f.great}" data-act="great"><span class="dot-great" aria-hidden="true"></span>${esc(t('Plays great'))}<span class="switch" aria-hidden="true"></span></button>
      ${onHandheld() ? `<button type="button" class="toggle deck-t" role="switch" aria-checked="${!!f.deckgreat}" data-act="deck-f">${DECK_ICON(15)}${esc(t('Great on Deck'))}<span class="switch" aria-hidden="true"></span></button>` : ''}
      <button type="button" class="toggle" role="switch" aria-checked="${!!f.hideEnded}" data-act="ended-f">${esc(t('Hide Ended'))}<span class="switch" aria-hidden="true"></span></button>
      <button type="button" class="toggle" role="switch" aria-checked="${!!S.settings.hide_ai}" data-act="toggle" data-k="hide_ai">${esc(t('Hide AI-assisted'))}<span class="switch" aria-hidden="true"></span></button>`;
const discGenres = () => { const apps = visible(); return Object.keys(GENRES).filter((k) => apps.some((a) => a.genre === k)); };
function pageDiscover() {
  wantCounts();
  if (!S.cat) return `<div class="loading">${esc(t('Loading…'))}</div>`;
  const f = S.find, apps = visible();
  const genres = Object.keys(GENRES).filter((k) => apps.some((a) => a.genre === k));
  const q = S.q.trim();
  return `<div class="page">${offlineNote()}
    <div class="dhead"><h1>${esc(q ? t('Search') : f.tag ? tf('All recomps with {tag}', { tag: f.tag }) : f.genre ? t(GENRES[f.genre]) : t('Discover'))}</h1><p>${esc(t('Every recomp comes straight from its developer\'s GitHub. You bring your own copy of the game.'))}</p>
      <label class="bigsearch"><span class="sr">${esc(t('Search recomps'))}</span>${SEARCH}<input id="dq" type="search" placeholder="${esc(t('Search games, recomps or developers'))}" value="${esc(S.q)}" autocomplete="off" spellcheck="false"><button type="button" class="clear" data-act="clear-q" aria-label="${esc(t('Clear the search'))}"${q ? '' : ' hidden'}>${XMARK}</button></label></div>
    ${notesBars()}
    <div class="dctl rows"><div class="grp">
      <div class="seg big" role="radiogroup" aria-label="${esc(t('Sort'))}">${sortOpts().map(([k, n]) => `<button type="button" role="radio" aria-checked="${f.sort === k}" data-act="sort" data-k="${k}">${esc(n)}</button>`).join('')}</div>
    </div><div class="grp">
      ${S.os === 'linux' ? runsSel() : ''}${originSel(f)}
      ${genreSel(f, genres)}
      ${showSel(f)}
      ${S.os === 'linux' ? '' : runsSel()}
      ${tagSel(f)}
    </div><div class="grp">
      ${discToggles(f)}
    </div></div>
    ${f.tag ? `<div class="chips tagchips"><button type="button" class="chip on" data-act="clear-tag" aria-label="${esc(t('Clear the search'))}">${esc(t('Tag'))}: ${esc(f.tag)} ${XMARK}</button></div>` : ''}
    <div id="dlist">${discoverGrid()}</div>
  </div>`;
}
// Discover's search and main filters, pinned to the top once the page's own scroll out of view, so
// they're always at hand further down. The rest are under Filters. Outside the page (in the window's
// frame), so a redraw of the page doesn't touch it, and only on Discover.
const SLIDERS = '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M4 7h9M17 7h3M4 17h3M11 17h9"/><circle cx="15" cy="7" r="2.2"/><circle cx="9" cy="17" r="2.2"/></svg>';
function stickExtras(f) { return [f.show, f.tag, f.easy, f.mp, f.great, onHandheld() && f.deckgreat, f.hideEnded, S.settings.hide_ai].filter(Boolean).length; }
function paintStick() {
  let b = document.getElementById('dstick');
  if (S.view !== 'discover' || !S.cat || !$('#dlist')) {
    if (b) { b.remove(); $('.mainwrap').classList.remove('stick-on'); }
    return;
  }
  if (!b) {
    b = document.createElement('div');
    b.id = 'dstick'; b.className = 'dstick'; b.inert = true;
    b.setAttribute('data-tauri-drag-region', '');
    b.innerHTML = `<div class="ds-in" data-tauri-drag-region>
      <label class="ds-q"><span class="sr">${esc(t('Search recomps'))}</span>${SEARCH}<input id="dqs" type="search" placeholder="${esc(t('Search games, recomps or developers'))}" autocomplete="off" spellcheck="false"><button type="button" class="clear" data-act="clear-q" aria-label="${esc(t('Clear the search'))}" hidden>${XMARK}</button></label>
      <div class="ds-ctl"></div>
      <div class="ds-more-w"><button type="button" class="ds-more" data-act="ds-more" aria-expanded="false" aria-haspopup="true">${SLIDERS}<span>${esc(t('Filter'))}</span><b class="ds-n" hidden></b></button><div class="ds-panel" hidden></div></div>
    </div>`;
    $('.mainwrap').append(b);
    b.dataset.lang = curLang();
  }
  const f = S.find, genres = discGenres();
  const key = JSON.stringify([curLang(), f, !!S.settings.hide_ai, runsPick(), (S.cat.origins || []).length, genres, allTags().length, S.os]);
  if (b.dataset.k !== key) {
    b.dataset.k = key;
    $('.ds-ctl', b).innerHTML = pillSel(t('Sort'), 'sort-sel', sortOpts(), f.sort) + originSel(f) + genreSel(f, genres, 'ds-wide');
    $('.ds-panel', b).innerHTML = `<div class="ds-list">${genreSel(f, genres, 'ds-narrow')}${showSel(f)}${runsSel()}${tagSel(f)}</div><div class="ds-list">${discToggles(f)}</div>`;
    const n = stickExtras(f), nb = $('.ds-n', b);
    nb.hidden = !n; nb.textContent = n ? fmtNum(n) : '';
    if (window.syncDrops) window.syncDrops();
  }
  const q = $('#dqs', b);
  if (document.activeElement !== q && q.value !== S.q) q.value = S.q;
  $('.clear', b).hidden = !S.q.trim();
  stickCheck();
}
// On once the page's own controls are above the top of the view.
function stickCheck() {
  const b = document.getElementById('dstick');
  if (!b) return;
  const c = $('#main .dctl');
  const on = !!c && c.getBoundingClientRect().bottom < $('#main').getBoundingClientRect().top + 12;
  if (b.classList.contains('on') === on) return;
  b.classList.toggle('on', on);
  b.inert = !on;
  $('.mainwrap').classList.toggle('stick-on', on);
  if (!on) {
    stickPanel(false);
    // Typing up there and scrolled back: the page's own field takes over.
    if (document.activeElement && document.activeElement.id === 'dqs') { const d = $('#dq'); if (d) { d.focus({ preventScroll: true }); d.setSelectionRange(d.value.length, d.value.length); } }
  }
}
function stickPanel(open) {
  const b = document.getElementById('dstick');
  if (!b) return;
  const p = $('.ds-panel', b), btn = $('.ds-more', b);
  if (open == null) open = p.hidden;
  p.hidden = !open;
  btn.setAttribute('aria-expanded', String(open));
}
// Every tag in the list, the most used first.
function allTags() {
  const n = {};
  for (const a of visible()) for (const tg of tagsOf(a)) n[tg] = (n[tg] || 0) + 1;
  return Object.keys(n).sort((x, y) => n[y] - n[x] || x.localeCompare(y));
}
// A page of Discover's grid. A thousand recomps drawn at once takes the window away for more than
// a second (inserting every tile, then laying them out); the next page comes as the bottom of the
// list nears, so the page opens at once and scrolling stays smooth.
const DISCOVER_PAGE = typeof IntersectionObserver === 'function' ? 120 : 1e9;
const MORE_IO = typeof IntersectionObserver === 'function' ? new IntersectionObserver((ents) => {
  if (ents.some((e) => e.isIntersecting)) moreDiscover();
}, { rootMargin: '1200px 0px' }) : null;
let MORE_AT = null;
function watchMore() {
  if (!MORE_IO) return;
  const el = S.view === 'discover' ? $('#dlist .more') : null;
  if (el === MORE_AT) return;
  if (MORE_AT) MORE_IO.unobserve(MORE_AT);
  MORE_AT = el && el.isConnected ? el : null;
  if (MORE_AT) MORE_IO.observe(MORE_AT);
}
function moreDiscover() {
  const box = $('#dlist'), grid = box && $('.grid', box);
  if (!grid) return;
  const list = discoverList();
  const from = Math.min(S.dshow || DISCOVER_PAGE, list.length), to = Math.min(from + DISCOVER_PAGE, list.length);
  if (to <= from) return;
  S.dshow = to;
  grid.insertAdjacentHTML('beforeend', list.slice(from, to).map((a) => tile(a, { quick: true })).join(''));
  if (to >= list.length) { const more = $('.more', box); if (more) more.remove(); }
  fitTitles(grid);
  watchMore();
}
function discoverGrid() {
  const f = S.find, list = discoverList();
  // Another search, filter or sort starts again from the first page.
  const key = JSON.stringify([S.q, S.find, S.settings.hide_ai, runsPick()]);
  if (S.dkey !== key) { S.dkey = key; S.dshow = DISCOVER_PAGE; }
  const shown = Math.min(S.dshow || DISCOVER_PAGE, list.length);
  const endedBox = f.hideEnded ? endedBoxHtml(discoverList(true)) : '';
  return list.length ? `<div class="count-line">${esc(tn(S.findCount || list.length, '1 recomp', '{n} recomps'))}</div><div class="grid" data-n="${list.length}" style="margin-top:20px">${list.slice(0, shown).map((a) => tile(a, { quick: true })).join('')}</div>${shown < list.length ? '<div class="more" aria-hidden="true"></div>' : ''}${endedBox}`
    : `<div class="empty"><h2>${esc(t('Nothing here'))}</h2><p>${esc(f.great ? t('Nothing here has enough player reports yet. Turn off “Plays great” to see the rest.') : f.deckgreat && onHandheld() ? t('No recomps match. Turn off “Great on Deck” to see the rest.') : f.easy ? t('No Easy Setup recomps match. Turn off “Easy Setup” to see the rest.') : f.mp ? t('No multiplayer recomps match. Turn off “Multiplayer” to see the rest.') : t('Try another search, console or genre.'))}</p></div>${endedBox}`;
}
// Opening "Ended projects" scrolls the page down with it, so the first row is in view. The box is still
// growing, so a single smooth scroll would stop at the page's old end: this follows it frame by frame
// until it has settled, and gives up the moment the player scrolls or presses anything themselves.
function revealEnded(head) {
  const main = $('#main');
  if (!main) return;
  const stick = 66 + 16, instant = calm(), ac = new AbortController();
  let last = performance.now();
  const until = last + 700;
  const stop = () => ac.abort();
  for (const ev of ['wheel', 'touchstart', 'keydown', 'pointerdown']) main.addEventListener(ev, stop, { passive: true, signal: ac.signal });
  const step = (now) => {
    if (ac.signal.aborted || !head.isConnected) return ac.abort();
    const want = head.getBoundingClientRect().top - main.getBoundingClientRect().top - stick;
    if (want > 1) main.scrollTop += instant ? want : Math.max(1, want * (1 - Math.pow(0.82, (now - last) / 16.7)));
    last = now;
    if (now < until) requestAnimationFrame(step); else ac.abort();
  };
  requestAnimationFrame(step);
}
// "Hide ended" doesn't lose them: they wait in a closed box under the list, with the covers greyed out.
function endedBoxHtml(list) {
  if (!list.length) return '';
  return `<section class="ended-box"><button type="button" class="ended-head" data-act="ended-box" aria-expanded="${!!S.endedOpen}" aria-controls="ended-body"><span>${esc(t('Ended Projects'))}</span><span class="n">${list.length}</span></button>`
    + `<div id="ended-body" class="ended-body${S.endedOpen ? ' open' : ''}"><div class="ended-in"><div class="ended-pad"><div class="grid ended-grid" data-n="${list.length}">${list.map((a) => tile(a, { quick: true })).join('')}</div></div></div></div></section>`;
}

// A Library filter whose games are all gone (the last update installed, the last copy set up,
// the last favorite removed): back to All, since its chip isn't there to switch back with.
function libFilterOk() {
  const f = S.lib.filter;
  const any = f === 'update' ? Object.keys(S.updates).some((id) => S.library[id])
    : f === 'setup' ? libRecs().some(needsCopySetup)
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
  const q = fold(S.lq.trim());
  let list = libRecs().map((g) => ({ g, a: libApp(g) })).concat(savedItems());
  if (S.lib.filter === 'saved') list = list.filter((x) => x.saved);
  if (S.lib.filter === 'inst') list = list.filter((x) => !x.saved);
  if (S.lib.filter === 'update') list = list.filter((x) => S.updates[parentId(x.g.id)]);
  // Installed games still waiting for the player's copy (as its count says); saved ones are under Not Installed.
  if (S.lib.filter === 'setup') list = list.filter((x) => !x.saved && needsCopySetup(x.g));
  if (S.lib.filter === 'fav') list = list.filter((x) => favs().includes(x.g.id));
  if (S.lib.origin) list = list.filter((x) => x.a.origin === S.lib.origin);
  if (q) list = list.filter((x) => fold([x.a.name, x.a.game].join(' ')).includes(q));
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
  const total = libRecs().length + nsaved;
  if (!total) {
    return `<div class="page"><div class="head"><div><h1>${esc(t('Library'))}</h1><p>${esc(t('Your recomps, kept up to date from each developer\'s own GitHub.'))}</p></div></div>
      <div class="empty"><svg width="44" height="44" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="4" y="4" width="6.5" height="6.5" rx="1.5"/><rect x="13.5" y="4" width="6.5" height="6.5" rx="1.5"/><rect x="4" y="13.5" width="6.5" height="6.5" rx="1.5"/><rect x="13.5" y="13.5" width="6.5" height="6.5" rx="1.5"/></svg>
      <h2>${esc(t('Nothing installed yet'))}</h2><p>${esc(t('Pick a recomp in Discover and install it. It shows up here, ready to play.'))}</p><div class="acts"><button type="button" class="btn" data-act="adopt">${esc(t('Bring Your Recomps'))}</button><button type="button" class="btn main" data-go="discover">${esc(t('Discover recomps'))}</button>${devOn() ? `<button type="button" class="btn" data-act="dev-add">${esc(t('Add a local build'))}</button>` : ''}</div></div></div>`;
  }
  const n = Object.keys(S.updates).filter((id) => S.library[id]).length;
  const setup = libRecs().filter(needsCopySetup).length;
  const nf = favs().length;
  // Consoles the player's games are from: a filter once there's more than one.
  const origins = [...new Set(Object.values(S.library).map((g) => (byId(g.id) || {}).origin).filter(Boolean))];
  const consoleSel = origins.length > 1 ? `<label class="pill-sel">${esc(t('Console'))}<select class="sel" data-act="liborigin">${[['', t('Any')], ...origins.map((o) => [o, originName(o)])].map(([k, nm]) => `<option value="${esc(k)}"${(S.lib.origin || '') === k ? ' selected' : ''}>${esc(nm)}</option>`).join('')}</select></label>` : '';
  return `<div class="page"><div class="head"><div><h1>${esc(t('Library'))}</h1><p>${esc(tn(total, '1 game. Right-click one for versions, launch options and more.', '{n} games. Right-click one for versions, launch options and more.'))}</p></div>
      <div class="dctl"><label class="smallsearch"><span class="sr">${esc(t('Search your games'))}</span>${SEARCH.replace('width="20" height="20"', 'width="16" height="16"')}<input id="lq" type="search" placeholder="${esc(t('Search your games'))}" value="${esc(S.lq)}" autocomplete="off" spellcheck="false"></label>
      <button type="button" class="btn" data-act="adopt">${esc(t('Bring Your Recomps'))}</button>
      <button type="button" class="btn" data-act="gh-add">${BRANCH_S}${esc(t('Add From GitHub or GitLab'))}</button>
      ${devOn() ? `<button type="button" class="btn" data-act="dev-add">${esc(t('Add a local build'))}</button>` : ''}
      ${consoleSel}<label class="pill-sel">${esc(t('Sort'))}<select class="sel" data-act="libsort"><option value="played"${S.lib.sort === 'played' ? ' selected' : ''}>${esc(t('Recently played'))}</option><option value="time"${S.lib.sort === 'time' ? ' selected' : ''}>${esc(t('Most played'))}</option><option value="az"${S.lib.sort === 'az' ? ' selected' : ''}>${esc(t('A to Z'))}</option></select></label></div></div>
    ${n || setup || nf || nsaved ? `<div class="chips"><button type="button" class="chip${S.lib.filter === 'all' ? ' on' : ''}" data-act="libf" data-f="all">${esc(t('All'))} <b>${fmtNum(total)}</b></button>${nsaved ? `<button type="button" class="chip${S.lib.filter === 'inst' ? ' on' : ''}" data-act="libf" data-f="inst">${esc(t('Installed'))} <b>${fmtNum(libRecs().length)}</b></button>` : ''}${n ? `<button type="button" class="chip${S.lib.filter === 'update' ? ' on' : ''}" data-act="libf" data-f="update">${esc(t('Update ready'))} <b>${fmtNum(n)}</b></button>` : ''}${setup ? `<button type="button" class="chip${S.lib.filter === 'setup' ? ' on' : ''}" data-act="libf" data-f="setup">${esc(t('Needs your copy'))} <b>${fmtNum(setup)}</b></button>` : ''}${nf ? `<button type="button" class="chip${S.lib.filter === 'fav' ? ' on' : ''}" data-act="libf" data-f="fav">${STAR}${esc(t('Favorites'))} <b>${fmtNum(nf)}</b></button>` : ''}${nsaved ? `<button type="button" class="chip${S.lib.filter === 'saved' ? ' on' : ''}" data-act="libf" data-f="saved">${esc(t('Not Installed'))} <b>${fmtNum(nsaved)}</b></button>` : ''}${n > 1 ? `<button type="button" class="chip main" data-act="update-all">${esc(tf('Update All ({n})', { n }))}</button>` : ''}</div>` : ''}
    <div id="llist" class="${libWide() ? 'wide' : ''}" style="--tile:${libTile()}">${libraryGrid()}</div>
  </div>`;
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
  // Covers the player made bigger than 200 pixels get the bigger copy, so they stay sharp. The
  // copies are 160, 400 and 640 wide: the one that covers the tile's real size on this screen.
  const px = libTile() * (devicePixelRatio || 1);
  const w = px > 420 ? 640 : px > 190 ? 400 : 0;
  return list.length ? `<div class="grid compact">${list.map((x) => x.saved ? tile(x.a, { wide, w, sub: esc(t('Not installed')) }) : tile(x.a, { wide, w, mine: true, quick: true, inst: x.n, sub: replacedBy(x.g.id) && !S.running.has(x.g.id) ? `<span class="tl-setup">${esc(tf('Replaced by {name}', { name: replacedBy(x.g.id).name }))}</span>` : x.a.parent && !S.running.has(x.g.id) ? gameTileLine(x.a) : esc(S.running.has(x.g.id) ? t('Playing now') : x.g.last_played ? tf('Played {when}', { when: ago(x.g.last_played) }) : t('Not played yet')) })).join('')}</div>` : `<div class="empty"><p>${esc(t('Nothing matches.'))}</p></div>`;
}




function pageGame(id) {
  wantCounts();
  let a = byId(id);
  if (!a && S.library[id]) a = libApp(S.library[id]);
  if (!a) return S.cat ? `<div class="page"><div class="empty"><h2>${esc(t('This recomp isn\'t listed anymore'))}</h2><button type="button" class="btn" data-go="discover">${esc(t('Back to Discover'))}</button></div></div>` : `<div class="loading">${esc(t('Loading…'))}</div>`;
  const inst = S.library[id];
  if (S.mods[id] === undefined) loadMods(id);
  if (inst && S.setup[id] === undefined) loadSetup(id);
  const su = S.setup[id];
  // Easy Setup first; the AI-assisted tag last, and only when the player turned it on (the details stay in Overview).
  const sp = statePill(a), quiet = sp.includes('tag quiet');
  const chips = [quiet ? '' : sp, turnkey(a) ? `<span class="tag cy">${DISC_S}${esc(t('Easy Setup'))}</span>` : '', greatOnDeck(a) ? `<span class="tag deck" title="${esc(t('Great on Deck: Steam Deck players say it plays great'))}">${DECK_ICON(14)}${esc(t('Great on Deck'))}</span>` : '', greatHandheld(a) ? `<span class="tag deck hh" title="${esc(tf('Handheld players say it runs well: {devices}', { devices: (Array.isArray(a.handheld.devices) ? a.handheld.devices : []).filter((d) => typeof d === 'string').join(', ') }))}">${HH_ICON}${esc(t('Great on Handhelds'))}</span>` : '', su && su.copy ? `<span class="tag">${esc(t('Your copy is set up'))}</span>` : '', inst && inst.local ? `<span class="tag cy">${esc(inst.tag === 'own' ? t('Added by You') : t('Local build'))}</span>` : '', lookChip(inst), quiet ? sp : '', isAI(a) && (S.settings.ui || {}).show_ai_tag ? `<span class="ai-tag">${esc(t('AI-assisted'))}</span>` : ''].join('');
  const kick = [GENRES[a.genre] ? t(GENRES[a.genre]) : '', originName(a.origin)].filter(Boolean).join(' · ');
  return `<div class="gp${a.hero || a.cover ? '' : ' noart'}" data-gp="${esc(id)}">
    <div class="gp-amb">${pic(thumbOf(a.cover, 160))}</div>
    <div class="gp-hero">${pic(a.hero || a.cover)}<button type="button" class="btn glass gp-back" data-act="back">${CHEV_L}${esc(t('Back'))}</button></div>
    <div class="gp-top"><div class="gp-cover">${pic(thumbOf(a.cover))}</div>
      <div class="gp-name">${kick ? `<span class="kick">${esc(kick)}</span>` : ''}<h1 class="${a.name.length > 34 ? 'long' : ''}">${esc(a.name)}</h1></div>
      <div class="gp-title">${partOfLine(a)}${based(a)}${devLine(a)}${verPill(a)}${chips ? `<div class="gp-chips">${chips}</div>` : ''}${tagsOf(a).length ? `<div class="gp-tags" aria-label="${esc(t('Tags'))}">${tagsOf(a).map((tg) => `<button type="button" class="tagpill" data-act="tag" data-tag="${esc(tg)}" title="${esc(tf('All recomps with {tag}', { tag: tg }))}">${esc(tg)}</button>`).join('')}</div>` : ''}</div>
      <div class="gp-side"><div class="gp-acts" data-acts="${esc(id)}">${actions(a)}</div>${replacedNote(a) || reinstallNote(a) || badNote(a) || stateNote(a)}</div></div>
    ${includesBox(a)}
    <div class="tabs"><div class="tabs-in" role="tablist"><span class="tab-pill" aria-hidden="true"></span>${gameTabs(a).map(([k, n, x]) => `<button type="button" role="tab" class="${S.tab === k ? 'on' : ''}" aria-selected="${S.tab === k}" data-act="tab" data-tab="${k}">${esc(n)}${x || ''}</button>`).join('')}</div></div>
    <div class="g-body" id="g-body">${gameBody(a)}</div></div>`;
}
// RecompLauncher itself in Steam, starting in Big Picture mode (Gaming Mode on a Steam Deck).
window.selfToSteam = selfToSteam;
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
// Each version keeps its own store icon in the versions list, even when Show Game Media or the
// player's pictures for every version make them all look alike: it's how they're told apart.
const verIcon = (v) => catalogArt(v, 'icon') || catalogArt(v, 'cover');
function verRow(v, cur, g, cls) {
  const inst = S.library[v.id], here = v.id === cur;
  const tags = [v.id === g.pick ? `<span class="vtag rec">${esc(t('Recommended'))}</span>` : '', inst ? `<span class="vtag inst">${esc(S.running.has(v.id) ? t('Playing') : t('Installed'))}</span>` : '', v.soon ? `<span class="vtag">${esc(t('Soon'))}</span>` : ''].join('');
  const line = [esc((devOf(v) || {}).name || v.author || '') + (devOf(v) && devOf(v).verified ? TICK(12) : ''), esc(v.adds || '')].filter(Boolean).join(' · ');
  const os = OS_SHORT(v);
  const ic = verIcon(v);
  return `<button type="button" class="${cls}${here ? ' on' : ''}" ${cls === 'vm-row' ? 'role="option" ' : ''}aria-${cls === 'vm-row' ? 'selected' : 'current'}="${here}" data-act="ver-go" data-id="${esc(v.id)}">
    <span class="v-ic">${ic ? pic(ic, '', 'loading="lazy"') : ''}</span>
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
  return `<div class="card vcard${hide ? ' shut' : ''}"><div class="vc-h"><span class="vc-stack">${g.list.slice(0, 4).map((v) => `<i>${verIcon(v) ? pic(thumbOf(verIcon(v), 160), '', 'loading="lazy"') : ''}</i>`).join('')}</span>
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
  return l && looksHere() ? `<span class="tag">${esc(tf('{name} shader', { name: t(l.name) }))}</span>` : '';
}
// A small "Beta" mark for features still being worked on (the launcher's own game settings).
const BETA_MARK = () => `<b class="tab-beta">${esc(t('Beta'))}</b>`;
function gameTabs(a) {
  const id = a.id, inst = S.library[id];
  const su = S.setup[id];
  const local = inst && inst.local && inst.tag !== 'own';
  const tabs = (local ? [['dev', t('Developer')], ['overview', t('Overview')]] : inst && inst.local ? [['overview', t('Overview')]] : [['overview', t('Overview')]].concat(a.repo ? [['updates', t('Updates')]] : [])).concat((su && su !== 'loading') || (su === 'loading' && inst && S.easy.has(id)) ? [['settings', t('Game settings'), su && su.beta && su.settings && su.settings.length ? BETA_MARK() : '']] : [], inst && looksHere() ? [['looks', t('Shaders')]] : [], S.mods[id] && S.mods[id] !== 'loading' ? [['mods', t('Mods')]] : [], inst ? [['saves', t('Saves')]] : [], inst ? [['installed', t('Manage')]] : [], window.achTabs ? window.achTabs(id) : [], window.pluginTabs ? window.pluginTabs(id) : []);
  if (!tabs.some(([k]) => k === S.tab)) S.tab = local ? 'dev' : 'overview';
  return tabs;
}
// ---------- how it runs, by the players' word ----------
const RUN_WORD = { runs: N_('Plays great'), issues: N_('Has problems'), broken: N_('Won\'t run') };
// What players say about each recomp: asked for when a page that shows it opens, at most once an
// hour (the core keeps it on disk for an hour too).
const COUNTS = { at: 0 };
function wantCounts() {
  if (Date.now() - COUNTS.at < 3600e3) return;
  COUNTS.at = Date.now();
  setTimeout(() => loadCounts(), 0);
}
async function loadCounts(force) {
  const before = JSON.stringify([S.runCounts || {}, S.deckScores || {}]);
  const [rc, ds] = await Promise.all([invoke('report_counts', { force: !!force }).catch(() => null), invoke('deck_scores', { force: !!force }).catch(() => null)]);
  S.runCounts = rc || S.runCounts || {};
  S.deckScores = ds || S.deckScores || {};
  // Nothing new: the page stays as it is (no second draw of a long list).
  if (JSON.stringify([S.runCounts, S.deckScores]) === before) return;
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
// Great on Deck: three or more Steam Deck players, and a score of 80 or more.
const greatOnDeck = (a) => { const d = a && (S.deckScores || {})[a.id]; return !!d && Number(d.n) >= 3 && Number(d.score) >= 80; };
const onHandheld = () => !!(S.deck || (S.handheld && S.os === 'linux'));
const DECK_ICON = (n = 13) => `<svg width="${n}" height="${n}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 11h4M8 9v4M15 12h.01M18 10h.01"/><path d="M17.3 5H6.7a4 4 0 0 0-4 3.6l-.9 7.2a2.6 2.6 0 0 0 2.6 2.9c1 0 1.7-.5 2.3-1.3L8 15.5h8l1.3 1.9c.6.8 1.3 1.3 2.3 1.3a2.6 2.6 0 0 0 2.6-2.9l-.9-7.2a4 4 0 0 0-4-3.6z"/></svg>`;
const BELL = (n = 18) => `<svg width="${n}" height="${n}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9"/><path d="M10.3 21a1.9 1.9 0 0 0 3.4 0"/></svg>`;

// ---------- Notify Me on Coming Soon games ----------
// The games the signed-in player asked to hear about: one email from the site when each comes out.
// `seq` goes up with every look and every press, so an answer that comes back after a newer one
// (or after a press) is left out.
const NOTIFY = { set: new Set(), pending: null, seq: 0 };
const notifyOn = (id) => NOTIFY.set.has(id);
async function loadNotifies() {
  const before = [...NOTIFY.set].sort().join();
  const seq = ++NOTIFY.seq;
  if (!(S.account && S.account.signedIn)) NOTIFY.set = new Set();
  else {
    try {
      const list = await invoke('notifies');
      if (seq !== NOTIFY.seq) return;
      NOTIFY.set = new Set(list || []);
    } catch (e) { /* kept as it was */ }
  }
  if ([...NOTIFY.set].sort().join() !== before) notifyRedraw();
}
function notifyRedraw() {
  if (S.view === 'game') refreshGame(); else if (S.view === 'discover' && $('#dlist')) morphList($('#dlist'), discoverGrid()); else render(true);
  if (window.tvRefresh && document.body.classList.contains('tv-on')) window.tvRefresh();
}
async function toggleNotify(id) {
  // Not signed in: the sign-in first, and it's turned on once that's done.
  if (!(S.account && S.account.signedIn)) { NOTIFY.pending = id; return signIn(false); }
  const on = !notifyOn(id);
  let r;
  try { r = await invoke('notify', { id, on }); } catch (e) { return fail(e); }
  NOTIFY.seq++;
  if (r) NOTIFY.set.add(id); else NOTIFY.set.delete(id);
  toast(r ? t('We\'ll email you when it comes out.') : t('You won\'t get an email about it.'));
  notifyRedraw();
}
window.toggleNotify = toggleNotify;
window.notifyOn = notifyOn;
// Just signed in (or the launcher started signed in): their list, and the one they pressed first.
// The sign-in they started for it was cancelled, refused or ran out: nothing turns on later.
window.notifyForget = () => { NOTIFY.pending = null; };
window.afterSignIn = async () => {
  await loadNotifies();
  if (!(S.account && S.account.signedIn)) return;
  const id = NOTIFY.pending;
  NOTIFY.pending = null;
  if (id && !notifyOn(id)) toggleNotify(id);
};

// ---------- notes from the RecompStore team, on top of Discover ----------
const NOTES = { list: [] };
async function loadNotes() {
  try { NOTES.list = (await invoke('team_notes')) || []; } catch (e) { NOTES.list = []; }
  if (S.view === 'discover') render(true);
  if (window.tvRefresh && document.body.classList.contains('tv-on')) window.tvRefresh();
}
setInterval(loadNotes, 6 * 3600e3);
const notesShown = () => { const closed = new Set(((S.settings.ui || {}).notes_closed || []).map(String)); return NOTES.list.filter((n) => !closed.has(String(n.id))); };
const httpsOnly = (u) => (/^https:\/\/\S+$/i.test(String(u || '')) ? String(u) : '');
function notesBars() {
  const list = notesShown();
  return list.length ? `<div class="tnotes">${list.map((n) => `<div class="tnote ${esc(n.tone || 'cyan')}" role="note"><span class="tn-l">RecompLauncher</span><p dir="auto">${esc(n.body)}</p>${httpsOnly(n.link) ? `<button type="button" class="btn sm" data-act="note-url" data-url="${esc(httpsOnly(n.link))}">${esc(n.linkText || t('Learn More'))}</button>` : ''}<button type="button" class="tn-x" data-act="note-x" data-n="${esc(n.id)}" aria-label="${esc(t('Close'))}" title="${esc(t('Close'))}">${XMARK}</button></div>`).join('')}</div>` : '';
}
async function closeNote(id) {
  const closed = ((S.settings.ui || {}).notes_closed || []).map(String).filter((x) => x !== String(id));
  await saveUi({ notes_closed: [...closed, String(id)].slice(-100) }, true);
  render(true);
  if (window.tvRefresh && document.body.classList.contains('tv-on')) window.tvRefresh();
}
window.notesShown = notesShown;
window.closeNote = closeNote;
window.httpsOnly = httpsOnly;
// The small score beside a version's name: how many say it plays great.
function runScore(id) {
  const r = runTally((S.runCounts || {})[id]);
  if (!r.n) return '';
  const p = r.pct('runs');
  return `<span class="vscore ${p >= 75 ? 'good' : p >= 45 ? 'mid' : 'low'}" title="${esc(`${tf('{pct}% plays great', { pct: p })} · ${tn(r.n, '1 report', '{n} reports')}`)}">${esc(tf('{pct}%', { pct: p }))}</span>`;
}
// A developer's answers under a player's report, marked Developer.
function devReplies(x) {
  const list = (x.replies || []).filter((r) => r.dev);
  return list.length ? `<div class="rep-replies">${list.map((r) => `<div class="rep-reply dev"><div class="who"><b>${esc(r.name || t('Developer'))}</b><span class="dev-badge">${esc(t('Developer'))}</span><small>${esc(Date.parse(r.when) ? ago(Date.parse(r.when) / 1000) : '')}</small></div><p dir="auto">${esc(r.text)}</p></div>`).join('')}</div>` : '';
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
  // Reports from Linux or a Steam Deck on a Windows build: played through Proton.
  const pxc = rep.proton_counts || (rep.items || []).filter(viaPx).reduce((c, x) => Object.assign(c, { [x.verdict]: (c[x.verdict] || 0) + 1 }), {});
  const px = runTally(pxc);
  const pxLine = px.n ? `<p class="rep-px">${esc(tf('Through Proton: {great} of {all} say it plays great', { great: fmtNum(px.runs), all: fmtNum(px.n) }))}</p>` : '';
  const items = (rep.items || []).slice(0, 3).map((x) => `<div class="rep"><div class="rep-top"><span class="rverdict v-${esc(x.verdict)}"><i aria-hidden="true"></i>${esc(t(RUN_WORD[x.verdict] || ''))}</span>${viaPx(x) ? `<span class="rep-pxtag">${esc(t('Through Proton'))}</span>` : ''}<small>${esc([x.version, x.when ? ago(Date.parse(x.when) / 1000 || x.when) : ''].filter(Boolean).join(' · '))}</small></div>
      ${x.specs ? `<p class="rep-pc">${esc(x.specs)}</p>` : x.pc ? `<p class="rep-pc">${esc([x.pc.gpu, x.pc.cpu, x.pc.ram ? tf('{n} GB', { n: x.pc.ram }) : '', x.pc.os].filter(Boolean).join(' · '))}</p>` : ''}
      ${x.note ? `<p class="rep-note" dir="auto">${esc(x.note)}</p>` : ''}${devReplies(x)}</div>`).join('');
  return `<div class="card runs">${head(tn(r.n, '1 report', '{n} reports'))}
    <div class="runs-top"><b class="runs-pct">${esc(tf('{pct}%', { pct: r.pct('runs') }))}</b><span>${esc(t('say it plays great'))}</span></div>
    ${runBar(r)}
    <div class="rlegend">${['runs', 'issues', 'broken'].map((k) => `<div class="v-${k}"><i aria-hidden="true"></i><span>${esc(t(RUN_WORD[k]))}</span><b>${fmtNum(r[k])}</b></div>`).join('')}</div>
    ${pxLine}${items ? `<div class="reps">${items}</div>` : ''}${acts}</div>`;
}
const viaPx = (x) => !!x && (x.proton === true || x.proton === 1 || x.proton === '1' || x.proton === 'true');

// The catalog's "players": PCs that installed or played it from RecompLauncher in the last 90 days.
function playersText(a) {
  const n = Number(a.players);
  return n > 0 ? tn(n, '1 PC, last 90 days', '{n} PCs, last 90 days') : '';
}
function gameRail(a) {
  const facts = [
    [t('Latest version'), a.version ? 'v' + String(a.version).replace(/^v/i, '') : ''], [t('Size'), a.size], [t('Latest release'), date(a.released)], [t('First release'), date(a.first)],
    [t('Releases'), a.releases ? fmtNum(a.releases) : ''], [t('Downloads'), a.downloads ? fmtN(a.downloads) : ''], [t('Played on RecompLauncher'), playersText(a)], [isLab(a.repo) ? t('Stars on GitLab') : t('Stars on GitHub'), a.stars && !isSF(a.repo) ? fmtN(a.stars) : ''], [t('License'), a.license], [t('By'), a.author], [t('Setup By'), OWNER_NAME[a.maintained_by] ? t(OWNER_NAME[a.maintained_by]) : ''],
  ].filter(([, v]) => v);
  const sys = (a.systems || []).filter((k) => OS_NAME[k]);
  return `<div class="stack">${quickActs(a)}${S.tab === 'overview' ? readyCard(a) : ''}<div class="card"><dl class="dl">${facts.map(([k, v]) => `<div><dt>${esc(k)}</dt><dd>${k === t('By') ? `<span class="who">${whoHTML(a)}</span>` : esc(v)}</dd></div>`).join('')}
      ${sys.length ? `<div><dt>${esc(t('Runs on'))}</dt><dd style="display:flex;gap:10px;align-items:center;justify-content:flex-end">${sys.map((k) => `<span style="display:inline-flex;gap:6px;align-items:center">${osIcon(k)}${OS_NAME[k]}</span>`).join('')}</dd></div>` : ''}</dl></div>
    </div>`;
}
// The recomp's README from its GitHub, folded away under its heading until it's opened. Loaded
// as the page opens (the launcher keeps a copy for a day), so it opens at once.
S.readme = {};
function readmeCard(a) {
  if (!a.repo || a.local) return '';
  const r = S.readme[a.repo];
  if (r === undefined) {
    S.readme[a.repo] = 'loading';
    invoke('repo_readme', { repo: a.repo }).then((t2) => { S.readme[a.repo] = t2 || null; if (S.view === 'game' && (byId(S.arg) || {}).repo === a.repo && S.tab === 'overview') refreshGame(); }).catch(() => { S.readme[a.repo] = null; });
    return '';
  }
  if (!r || r === 'loading') return '';
  const open = !!S.readmeOpen;
  return `<div class="card rdm${open ? ' open' : ''}"><button type="button" class="rdm-h" data-act="readme" aria-expanded="${open}"><h3>${esc(t('README'))}</h3><span>${esc(isLab(a.repo) ? t('From its GitLab') : t('From its GitHub'))}</span>${CHEV_R}</button><div class="rdm-b md">${md(r)}</div></div>`;
}
function gameBody(a) {
  const id = a.id, inst = S.library[id];
  gameTabs(a);
  let body = '';
  if (String(S.tab).startsWith('plug-') && window.pluginTabBody) return window.pluginTabBody(S.tab, id) + gameRail(a);
  if (S.tab === 'ach' && window.achBody) return window.achBody(a) + gameRail(a);
  if (S.tab === 'overview') {
    const feats = (a.features && a.features.length ? a.features : a.tags || []);
    const stats = inst
      ? [[t('Played'), playedFor(inst)], [t('Last played'), inst.last_played ? ago(inst.last_played).replace(/^./, (c) => c.toLocaleUpperCase(curLocale())) : t('Not yet')], [t('Version'), (inst.tag || t('Not known')) + (S.updates[id] ? ' · ' + t('update ready') : '')], [t('Installed'), date(inst.installed_at * 1000)]]
      : [[t('Released'), date(a.released) || '—'], [t('Downloads'), a.downloads ? fmtN(a.downloads) : '—'], [t('Stars'), a.stars ? fmtN(a.stars) : '—'], [t('Size'), a.size || '—']];
    body = `<div class="stack"><div class="stats">${stats.map(([k, v]) => `<div><span>${esc(k)}</span><b>${esc(v)}</b></div>`).join('')}</div>
      ${inst ? playCard(inst) : ''}
      ${window.betaCard ? window.betaCard(a) : ''}
      ${a.blurb ? `<p class="blurb">${esc(a.blurb)}</p>` : ''}
      ${ownAboutCard(id)}
      ${versionsCard(a)}
      ${feats.length ? `<div class="feat">${feats.map((ft) => `<span>${esc(ft)}</span>`).join('')}</div>` : ''}
      ${aboutCard(a)}
      ${shotsCard(a)}
      ${runsCard(a)}
      ${a.description ? `<div class="card"><h3 style="margin-bottom:12px">${esc(t('From the developer'))}</h3><div class="md">${md(a.description)}</div></div>` : ''}
      ${readmeCard(a)}
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
      <div class="card">${r.list.filter((x) => !x.draft).slice(0, 10).map((x, n) => `<div class="rel"><div class="rel-h"><h3>${esc(x.name || x.tag_name)}</h3><span class="when">${esc(date(x.published_at))}</span>${x.prerelease ? `<span class="pre">${esc(t('Pre-release'))}</span>` : ''}${relState(id, inst, r.list, x, n)}</div><div class="md">${x.body ? md(x.body) : `<p class="muted">${esc(t('No notes for this one.'))}</p>`}</div></div>`).join('')}</div></div>`;
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
      <div class="set-row"><div class="k"><b>${esc(t('Project association'))}</b><span>${esc(inst.local ? t('Untracked · no updates') : inst.repo)}</span></div><button type="button" class="btn" data-act="project-association" data-id="${esc(id)}">${esc(t('Change…'))}</button></div>
      ${inst.local ? '' : `<div class="set-row"><div class="k"><b>${esc(t('Update automatically'))}</b><span>${esc(t('Installs new releases as soon as the launcher finds them. Your saves are backed up first.'))}</span></div><button type="button" class="switch" role="switch" aria-checked="${!!inst.auto_update}" aria-label="${esc(t('Update automatically'))}" data-act="auto-up" data-id="${esc(id)}"></button></div>`}
      <div class="set-row"><div class="k"><b>${esc(t('Launch options'))}</b><span>${inst.args ? esc(inst.args) : esc(t('Extra options passed to the game when it starts.'))}</span></div><button type="button" class="btn" data-act="launch-opts" data-id="${esc(id)}">${esc(inst.args ? t('Change') : t('Add'))}</button></div>
      <div class="set-row"><div class="k"><b>${esc(t('Program it starts'))}</b><span>${inst.exe ? esc(inst.exe) : esc(t('Not found. Pick it so Play knows what to start.'))}</span></div><button type="button" class="btn" data-act="setexe" data-id="${esc(id)}">${esc(t('Choose Program'))}</button></div>
      ${inst.local ? '' : `<div class="set-row"><div class="k"><b>${esc(t('Backups of your saves'))}</b><span>${esc(t('Taken before every update. Saves, settings and mods in its folder.'))}</span></div><button type="button" class="btn" data-act="backups" data-id="${esc(id)}">${esc(t('Backups'))}</button></div>`}
      ${inst.local ? '' : `<div class="set-row"><div class="k"><b>${esc(t('Installed version'))}</b><span>${inst.asset ? `${esc(inst.tag)} · ${esc(inst.asset)}${inst.size ? ' · ' + fmtB(inst.size) : ''} · ${esc(inst.verified ? (isLab(inst.repo) || isSF(inst.repo) ? t('matched its release\'s checksum') : t('matched GitHub\'s checksum')) : (isLab(inst.repo) || isSF(inst.repo) ? t('Its release had no checksum for it') : t('GitHub had no checksum for it')))}` : esc(inst.tag || t('Not known. Updating puts the newest one in its folder.'))}</span></div></div>`}
      ${inst.local ? `<div class="set-row"><div class="k"><b>${esc(t('Remove from Library'))}</b><span>${esc(t('Your folder stays exactly as it is.'))}</span></div><button type="button" class="btn danger" data-act="dev-remove" data-id="${esc(id)}">${esc(t('Remove'))}</button></div>` : `<div class="set-row"><div class="k"><b>${esc(t('Uninstall'))}</b><span>${esc(t('Takes it out of your Library. You choose whether its folder goes too.'))}</span></div><button type="button" class="btn danger" data-act="uninstall" data-id="${esc(id)}">${esc(t('Uninstall'))}</button></div>`}
    </div>
    <div class="card"><div class="card-h" style="margin-bottom:14px"><h3>${esc(t('Screenshots'))}</h3><button type="button" class="btn sm" data-act="shots-folder" data-id="${esc(id)}">${esc(t('Open folder'))}</button></div>
      ${shots && shots.length ? `<div class="shots">${shots.slice(0, 8).map((f, i) => `<button type="button" class="shot" data-act="shot-g" data-id="${esc(id)}" data-i="${i}" data-path="${esc(f)}"><img src="${esc(fileSrc(f))}" alt="${esc(t('Screenshot'))}" loading="lazy"${LOADED.has(fileSrc(f)) ? ' class="ld"' : ''}></button>`).join('')}</div>` : `<p class="muted" style="margin:0">${esc(S.os === 'macos' ? t('In-game screenshot capture is not available on Mac yet.') : t('None yet. While you play, press F12, or View + RB on a controller.'))}</p>`}</div></div>`;
  }
  return body + gameRail(a);
}
// What each release in the Updates tab offers, by the same rule as the Library's badge and
// Settings > Check (update_info): the update on offer, releases the store is still checking,
// and releases with no build for this system.
function relState(id, inst, list, x, n) {
  if (!inst) return '';
  if (inst.tag === x.tag_name) return `<span class="ai-tag inst">${esc(t('Installed'))}</span>`;
  const info = (S.updInfo || {})[id];
  if (info === undefined) { S.updInfo = S.updInfo || {}; S.updInfo[id] = null; invoke('update_info', { id }).then((v) => { S.updInfo[id] = v; if (S.view === 'game' && S.arg === id && S.tab === 'updates') showTab('updates'); }).catch(() => { S.updInfo[id] = false; }); }
  if (info && Array.isArray(info.builds) && !info.builds.includes(x.tag_name)) return `<span class="pre">${esc(t('No build for your system'))}</span>`;
  if (S.jobs[id] || S.running.has(id)) return '';
  const btn = (label, main) => `<button type="button" class="btn sm${main ? ' main' : ''}" data-act="version" data-id="${esc(id)}" data-tag="${esc(x.tag_name)}">${esc(label)}</button>`;
  if (info && info.offered === x.tag_name) return btn(t('Update'), true);
  const ci = info && info.checked ? list.findIndex((y) => y.tag_name.replace(/^v/i, '').toLowerCase() === info.checked.replace(/^v/i, '').toLowerCase()) : -1;
  const li = list.indexOf(x);
  if (ci > 0 && li < ci) return `<span class="pre">${esc(t('Still being checked'))}</span>${btn(t('Install Anyway'))}`;
  return btn(n === 0 ? t('Install this one') : t('Go back to this one'));
}
// Switching tabs redraws only the part below them: the banner and cover stay where they are.
function showTab(k) {
  const id = S.arg, a = byId(id) || (S.library[id] && libApp(S.library[id]));
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
  const id = S.arg, a = byId(id) || (S.library[id] && libApp(S.library[id]));
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
  [/^(switch|switch2|nx|nx2)$/, ['xci', 'nsp', 'nup', 'nca']],
  [/^(genesis|md|megadrive|gen)$/, ['md', 'gen', 'smd', 'bin', 'zip']],
  [/^(32x|sega32x)$/, ['32x', 'bin', 'zip']],
  [/^(gg|gamegear)$/, ['gg', 'zip']],
  [/^(sms|mastersystem)$/, ['sms', 'zip']],
  [/^(arcade|mame|fbneo)$/, ['zip', '7z', 'chd']],
  [/^(cdi|cd-i|philipscdi)$/, ['chd', 'cue', 'bin', 'iso']],
  [/^(amiga|amigacd32|cd32)$/, ['adf', 'hdf', 'ipf', 'lha', 'dms', 'zip']],
];
const extsFor = (origin) => { const o = String(origin || '').toLowerCase().replace(/[\s_.]/g, ''); const m = SYSTEM_EXTS.find(([re]) => re.test(o)); return m ? m[1] : null; };
// What the player's copy of a game can be, to show before they pick it: a cartridge console's own
// files (.nes, .sfc…), an N64 ROM in any byte order, or a disc image (.iso, plus the kinds its setup
// names, like .rvz), and a copied folder where it takes one.
const DISC_SHOWN = ['iso', 'xiso', 'gcm', 'rvz', 'wbfs', 'wia', 'gcz', 'nkit', 'chd', 'cso', 'ciso', 'img', 'bin', 'cue', 'pbp'];
function copyFormats(id) {
  const a = byId(id) || { id };
  const su = S.setup[id] && S.setup[id] !== 'loading' ? S.setup[id] : {};
  const folder = su.kind === 'folder' || su.kind === 'iso_or_folder';
  const own = extsFor(a.origin);
  if (own) return { type: 'file', exts: own, folder: false, console: originName(a.origin) };
  const b = (a && a.boot) || builtinBoot()[id] || {};
  const what = `${(b.files || {}).what || ''} ${su.needs || ''}`;
  if (/\bN64\b|\.z64|\.n64|\.v64|\bROM\b/i.test(what) && !/\bdisc\b|\.iso/i.test(what)) return { type: 'rom', exts: ['z64', 'n64', 'v64', 'zip', '7z'], folder: false };
  if (su.kind === 'folder') return { type: 'folder', exts: [], folder: true };
  if (/\bdisc\b|\.iso\b|\biso\b|\bxex\b/i.test(what) || su.kind === 'iso' || folder) {
    const named = [...what.matchAll(/\.([a-z0-9]{2,5})\b/gi)].map((m) => m[1].toLowerCase()).filter((e) => DISC_SHOWN.includes(e));
    return { type: 'disc', exts: [...new Set(['iso', ...named])], folder };
  }
  return { type: 'any', exts: [], folder };
}
// Those, as small tags (".z64 .n64 .v64"), the same in every language.
const fmtTags = (exts) => exts.map((e) => `<span class="fmt">.${esc(e)}</span>`).join('');
// The pick button's name, and the line under the drop area, for that kind of copy.
const copyName = (f) => (f.type === 'rom' ? t('ROM File') : f.type === 'disc' ? t('Disc Image') : f.type === 'file' ? tf('{console} File', { console: f.console }) : t('Game File'));
const copyDrop = (f) => (f.type === 'rom' ? t('Drop your ROM here') : f.type === 'disc' ? t('Drop your disc image here') : t('Drop your game file here'));
function copyPickOpts(id) {
  const a = byId(id) || { id };
  // A console with files of its own (cartridge ROMs, Switch games, Amiga disks): those.
  const own = extsFor(a.origin);
  if (own) return { title: t('Your copy of the game'), directory: false, multiple: false, filters: [{ name: tf('{console} files', { console: originName(a.origin) }), extensions: own }, { name: t('All files'), extensions: ['*'] }] };
  const b = (a && a.boot) || builtinBoot()[id] || {};
  const su = S.setup[id] && S.setup[id] !== 'loading' ? S.setup[id] : {};
  const what = `${(b.files || {}).what || ''} ${su.needs || ''}`;
  const n64 = /\bN64\b|\.z64|\.n64|\.v64|\bROM\b/i.test(what) && !/\bdisc\b|\.iso/i.test(what);
  const exts = n64 ? ROM_EXTS.n64 : ROM_EXTS.disc;
  // A file that's neither a ROM nor a disc (a download package with no extension): any file.
  if (!n64 && !/\bdisc\b|\.iso\b|\biso\b/i.test(what)) return { title: t('Your copy of the game'), directory: false, multiple: false, filters: [{ name: t('All files'), extensions: ['*'] }] };
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
function needsCopySetup(g) {
  // One game of several whose own inputs aren't set up yet (the others still play).
  if (g._sub && g.ready === false) return true;
  const su = S.setup[g.id];
  const named = su && su !== 'loading' && su.inputs?.length ? !su.inputs_ready : Object.keys(g.setup_inputs || {}).length > 0 && !g.inputs_ready;
  return S.easy.has(g.id) && (!g.game_copy || named);
}
function needUpdate(su) { return !!(su && ((su.inputs?.length && !su.inputs_ready) || su.inputs_error || (su.update && !su.update_copy && !su.update_optional))); }
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
const suDrive = createSetupDriveLight();
async function setupModal(id) {
  if (document.body.classList.contains('tv-on') && window.tvSetup) { S.setupNote = null; return window.tvSetup(id); }
  if (SU.el && SU.stage === 'run' && SU.id !== id) return toast(t('Finish the setup that\'s running first.'));
  if (S.setup[id] === undefined || S.setup[id] === 'loading') { try { S.setup[id] = await invoke('game_setup', { id }); } catch (e) { S.setup[id] = null; } }
  const su = S.setup[id];
  if (!su || !su.needs) return toast(t('This game asks for your copy itself the first time it starts.'));
  if (su.inputs?.length || su.inputs_error) return namedSetup(id);
  closeSetup(true);
  Object.assign(SU, { id, stage: needUpdate(su) && su.copy ? 'update' : 'pick', step: '', ev: {}, steps: {}, q: [], at: 0, path: '', extras: false, result: null, err: '', uerr: '', note: S.setupNote && S.setupNote.id === id ? S.setupNote.text : '' });
  S.setupNote = null;
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
  suDrive.stop();
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
    case 'again': if (S.setup[id]?.inputs?.length) return namedSetup(id); SU.stage = 'pick'; SU.err = ''; return suDraw();
    case 'anyway': return suBegin(SU.path, true);
    case 'manual': { const gid = SU.id; closeSetup(true); return manualModal(gid); }
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
async function suBegin(path, anyway) {
  // The catalog lists the discs or cartridges it's made for, and this one reads as another.
  if (!anyway) {
    let w = null;
    try { w = await invoke('copy_check', { id: SU.id, path }); } catch (e) { /* can't tell: go ahead */ }
    if (!SU.id) return;
    if (w) { Object.assign(SU, { stage: 'wrong', path, wrong: w }); return suDraw(); }
  }
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
    if (ev.step === 'error') { SU.q = []; SU.stage = 'error'; SU.err = ev.text; profileSaid(SU.id, 'files', false, ev.text); return suDraw(); }
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
    if (ev.step === 'done') { SU.result = ev; profileSaid(SU.id, 'files', true); return suAfterCopy(); }
  }
  suDraw();
}
T.event.listen('setup', ({ payload: p }) => { if (SU.el && p.id === SU.id && SU.stage === 'run') { SU.q.push(p); suPump(); } });
function suDraw() {
  if (!SU.el) return;
  if (SU.stage !== 'run') suDrive.stop();
  const id = SU.id, a = byId(id) || { name: (S.library[id] || {}).name || id }, su = S.setup[id] || {};
  const box = $('.modal', SU.el);
  const game = a.game || a.name;
  const cov = a.cover ? media(a.cover) : '';
  const discCls = SU.stage === 'pick' ? 'sdisc ghost' : SU.stage === 'run' ? (SU.step === 'read' && Date.now() - SU.at < 900 ? 'sdisc in' : 'sdisc spin') : SU.stage === 'ready' || SU.stage === 'update' ? 'sdisc done' : 'sdisc';
  const label = `<div class="lab">${cov ? pic(thumbOf(a.cover, 160)) : ''}</div><div class="sheen"></div><div class="hub"></div><div class="hole"></div>`;
  let right = '';
  if (SU.stage === 'pick') {
    const own = S.owned[id];
    const kind = su.kind || 'iso', fm = copyFormats(id);
    const picks = [kind !== 'folder' ? `<button type="button" class="pick" data-su="iso">${DISC_L}<b>${esc(copyName(fm))}</b><span class="fmts">${fm.exts.length ? fmtTags(fm.exts) : esc(t('The file you made from your own copy'))}</span></button>` : '', kind === 'iso_or_folder' || kind === 'folder' ? `<button type="button" class="pick" data-su="folder"><svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" aria-hidden="true"><path d="M3 6.5A1.5 1.5 0 0 1 4.5 5H10l2 2.5h7.5A1.5 1.5 0 0 1 21 9v9.5a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 18.5z"/></svg><b>${esc(t('Game folder'))}</b><span>${esc(t('A folder you copied from the disc'))}</span></button>` : ''].filter(Boolean);
    right = `<span class="kick">${esc(t('Set up'))} · ${esc(a.name)}</span><h2>${esc(tf('Insert your copy of {game}', { game }))}</h2><p>${esc(su.needs || '')} ${esc(t('It\'s checked and set up once. After that, you just press Play.'))}</p>
      ${SU.note ? `<p class="su-note" role="alert">${esc(SU.note)}</p>` : ''}
      ${own ? `<div class="found">${DISC_L}<div class="k"><b>${esc(t('Found in your folders'))}</b><span>${esc(own.path)}</span></div><button type="button" class="btn sm main" data-su="found">${esc(t('Use this'))}</button></div>` : ''}
      <div class="picks${picks.length === 1 ? ' one' : ''}">${picks.join('')}</div>`;
  } else if (SU.stage === 'run') {
    const order = ['read', 'verify'].concat(SU.extras ? ['extras'] : [], ['setup']);
    const st = SU.steps;
    const sub = {
      read: st.read && st.read.text ? st.read.text : baseName(SU.path),
      verify: st.verify && st.verify.text ? st.verify.text : st.verify && st.verify.total ? tf('{done} of {total}', { done: fmtB(st.verify.done), total: fmtB(st.verify.total) }) : t('Reading all of it, to make sure it\'s complete'),
      extras: (st.extras && st.extras.text) || t('What the game needs from your copy'),
      setup: t('Its settings, your controller and where your copy is'),
    };
    const names = { read: t('Reading your copy'), verify: t('Checking your copy'), extras: t('Getting the extras'), setup: t('Setting it up') };
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
  } else if (SU.stage === 'wrong') {
    const w = SU.wrong || {};
    right = `<span class="kick">${esc(t('Set up'))} · ${esc(a.name)}</span><h2>${esc(t('This Looks Like Another Copy'))}</h2><p class="su-err">${esc(tf('Your copy reads as {found}. This recomp is made for {want}.', { found: w.found || '', want: (w.want || []).join(', ') }))}</p>
      <p>${esc(t('Another region or version of the game may not work with it.'))}</p>
      <div class="picks" style="margin-top:auto"><button type="button" class="btn main big" data-su="again">${esc(t('Try another copy'))}</button><button type="button" class="btn big" data-su="anyway">${esc(t('Use It Anyway'))}</button></div>`;
  } else {
    right = `<span class="kick">${esc(t('Set up'))} · ${esc(a.name)}</span><h2>${esc(t('That copy didn\'t work'))}</h2><p class="su-err">${esc(t(SU.err))}</p>
      <div class="picks" style="margin-top:auto"><button type="button" class="btn main big" data-su="again">${esc(t('Try another copy'))}</button><button type="button" class="btn big" data-su="close">${esc(t('Close'))}</button></div>
      ${S.library[SU.id] ? `<p class="su-hand">${esc(t('Still not working?'))} <button type="button" class="link-btn" data-su="manual">${esc(t('Set it up by hand'))}</button></p>` : ''}`;
  }
  const keepDisc = $('.sdisc', box);
  const stageHtml = SU.stage === 'pick' ? `<div class="su-drop"><div class="sdisc ghost"></div><b>${esc(copyDrop(copyFormats(SU.id)))}</b><span>${esc(t('or choose it on the right'))}</span></div>` : `<div class="${discCls}">${label}</div>`;
  if (!box.firstChild || box.dataset.stage !== SU.stage) {
    if (SU.stage !== 'run') box.dataset.step = '';
    box.dataset.stage = SU.stage;
    box.innerHTML = `${cov ? `<div class="su-glow" style="background-image:url('${cssUrl(cov)}')"></div>` : ''}<button type="button" class="su-x" data-su="close" aria-label="${esc(t('Close'))}">${XMARK}</button>
      <div class="su-in"><div class="su-stage">${stageHtml}<div class="drive" aria-hidden="true"><i></i></div></div><div class="su-r">${right}</div></div>`;
    const f = $('.su-r .btn.main, .su-r .pick, .su-r .btn', box) || $('.su-x', box);
    if (f) f.focus({ preventScroll: true });
  } else {
    $('.su-r', box).innerHTML = right;
    const d = $('.sdisc', box);
    if (d && keepDisc && SU.stage === 'run' && SU.step !== 'read' && !d.classList.contains('spin')) d.className = 'sdisc spin';
  }
  if (SU.stage === 'run') suDrive.start($('.drive i', box));
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
  // A value the game set itself that isn't one of the choices: none is on, and the value shows.
  const seg = (o) => o.choices.length <= 4 && o.choices.join('').length <= 34
    ? `<div class="seg" role="radiogroup" aria-label="${esc(o.label)}">${o.choices.map((c, i) => `<button type="button" role="radio" aria-checked="${o.own == null && o.current === i}" data-act="gset" data-id="${esc(id)}" data-k="${esc(o.id)}" data-i="${i}"${o.applies ? '' : ' disabled'}>${esc(c)}</button>`).join('')}</div>`
    : `<select class="sel" data-act="gsel" data-id="${esc(id)}" data-k="${esc(o.id)}"${o.applies ? '' : ' disabled'}>${o.own != null ? `<option value="" selected disabled>${esc(o.own)}</option>` : ''}${o.choices.map((c, i) => `<option value="${i}"${o.own == null && o.current === i ? ' selected' : ''}>${esc(c)}</option>`).join('')}</select>`;
  return `<div class="stack">
    ${su.needs ? `<div class="card"><div class="set-row"><div class="k"><b>${esc(t('Your copy of the game'))}</b><span>${su.copy ? esc(su.copy) : esc(su.needs)}</span></div><button type="button" class="btn${su.copy ? '' : ' main'}" data-act="setup" data-id="${esc(id)}">${esc(su.copy ? t('Change') : t('Insert it'))}</button></div></div>` : ''}
    ${su.update ? `<div class="card"><div class="set-row"><div class="k"><b>${esc(t('The game\'s update'))}</b><span>${su.update_copy ? esc(su.update_copy) : esc(su.update)}</span></div><button type="button" class="btn${su.update_copy || su.update_optional ? '' : ' main'}" data-act="setupdate" data-id="${esc(id)}">${esc(su.update_copy ? t('Change') : t('Choose It'))}</button></div></div>` : ''}
    ${su.settings.length ? `<div class="card"><div class="card-h"><h3>${esc(t('How it runs'))}</h3><span>${esc(su.waiting ? t('Play it once first, then change them here.') : t('Saved in the game\'s own settings'))}</span></div>
      ${gameSettingRows(id, su, seg)}</div>` : ''}
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
// A game's settings in their order: a heading where the group changes, advanced ones only after
// Show Advanced Options (remembered per game), and sliders with their value beside them.
const advOpen = (id) => !!(((S.settings.ui || {}).adv_open || {})[id]);
function gameSettingRows(id, su, seg) {
  const open = advOpen(id), hasAdv = su.settings.some((o) => o.advanced);
  let last = '';
  const rows = su.settings.filter((o) => open || !o.advanced).map((o) => {
    const g = o.group || '', head = g && g !== last ? `<div class="set-sub">${esc(g)}</div>` : '';
    last = g;
    if (o.bindings) return head + bindRows(id, o);
    const ctl = o.slider ? `<div class="gslider"><input type="range" min="${o.slider.min}" max="${o.slider.max}" step="${o.slider.step}" value="${esc(parseFloat(o.slider.value) || o.slider.min)}" data-gslide data-id="${esc(id)}" data-k="${esc(o.id)}" aria-label="${esc(o.label)}"${o.applies ? '' : ' disabled'}><output>${esc(o.slider.value)}</output></div>` : seg(o);
    const help = o.own != null ? [o.help, tf('Set in the game to {value}.', { value: o.own })].filter(Boolean).join(' ') : o.help;
    return `${head}<div class="set-row${o.applies ? '' : ' off'}"><div class="k"><b>${esc(o.label)}</b><span>${esc(help)}</span></div>${ctl}</div>`;
  }).join('');
  return rows + (hasAdv ? `<div class="set-adv"><button type="button" class="btn" data-act="gadv" data-id="${esc(id)}">${esc(open ? t('Hide Advanced Options') : t('Show Advanced Options'))}</button></div>` : '');
}
// ---------- key bindings: each action's keys as chips; pressing one waits for the next key ----------
const KEY_CODES = { Space: 'Space', Enter: 'Return', NumpadEnter: 'Return', Tab: 'Tab', Backspace: 'Backspace', ShiftLeft: 'Shift', ShiftRight: 'Shift', ControlLeft: 'Ctrl', ControlRight: 'Ctrl', AltLeft: 'Alt', AltRight: 'Alt', ArrowUp: 'Up', ArrowDown: 'Down', ArrowLeft: 'Left', ArrowRight: 'Right', PageUp: 'Page Up', PageDown: 'Page Down', CapsLock: 'Caps Lock' };
// A key by where it is on the keyboard (Z is Z whatever the layout types), in the launcher's names.
function keyName(e) {
  const c = e.code || '';
  if (/^Key[A-Z]$/.test(c)) return c.slice(3);
  if (/^Digit\d$/.test(c)) return c.slice(5);
  if (/^F(?:[1-9]|1[0-2])$/.test(c)) return c;
  if (KEY_CODES[c]) return KEY_CODES[c];
  if (/^Numpad\d$/.test(c)) return 'Numpad ' + c.slice(6);
  if (c && !/^(?:Meta|OS|Unidentified)/.test(c)) return c;
  return e.key && e.key.length === 1 && e.key.trim() ? e.key.toUpperCase() : '';
}
const MOUSE_NAMES = ['Mouse Left', 'Mouse Middle', 'Mouse Right', 'Mouse 4', 'Mouse 5'];
// Controller buttons, Xbox and PlayStation names.
const PAD_NAMES = { a: ['A', 'Cross'], b: ['B', 'Circle'], x: ['X', 'Square'], y: ['Y', 'Triangle'], lb: ['LB', 'L1'], rb: ['RB', 'R1'], lt: ['LT', 'L2'], rt: ['RT', 'R2'], ls: ['LS', 'L3'], rs: ['RS', 'R3'], view: ['Back', 'Share'], menu: ['Start', 'Options'], up: ['D-Pad Up', 'D-Pad Up'], down: ['D-Pad Down', 'D-Pad Down'], left: ['D-Pad Left', 'D-Pad Left'], right: ['D-Pad Right', 'D-Pad Right'] };
let BIND = null;
// Waits for the next key, mouse button or wheel turn (and controller button, for an action that
// takes one). Escape cancels, and so does waiting ten seconds. done(name), or done(null).
function bindCapture(pad, done) {
  if (BIND) BIND.stop(null);
  let over = false, until = 0, timer = 0;
  const onKey = (e) => { e.preventDefault(); e.stopImmediatePropagation(); if (e.repeat) return; if (e.key === 'Escape') return stop(null); const n = keyName(e); if (n) stop(n); };
  const onDown = (e) => { e.preventDefault(); e.stopImmediatePropagation(); const n = MOUSE_NAMES[e.button]; if (n) stop(n); };
  const onWheel = (e) => { e.preventDefault(); e.stopImmediatePropagation(); if (e.deltaY) stop(e.deltaY < 0 ? 'Wheel Up' : 'Wheel Down'); };
  // The mouse button that was the answer doesn't also press what's under it.
  const eat = (e) => { if (!over || performance.now() < until) { e.preventDefault(); e.stopImmediatePropagation(); } };
  const EATS = ['mouseup', 'click', 'auxclick', 'contextmenu', 'dblclick'];
  function stop(name) {
    if (over) return;
    over = true;
    until = performance.now() + 400;
    clearTimeout(timer);
    window.removeEventListener('keydown', onKey, true);
    window.removeEventListener('mousedown', onDown, true);
    window.removeEventListener('wheel', onWheel, { capture: true });
    setTimeout(() => { for (const ev of EATS) window.removeEventListener(ev, eat, true); }, 450);
    BIND = null;
    window.bindPad = null;
    done(name);
  }
  for (const ev of EATS) window.addEventListener(ev, eat, true);
  // The press that started it is over before this listens.
  setTimeout(() => { if (over) return; window.addEventListener('keydown', onKey, true); window.addEventListener('mousedown', onDown, true); window.addEventListener('wheel', onWheel, { capture: true, passive: false }); }, 0);
  timer = setTimeout(() => stop(null), 10000);
  window.bindPad = (b, kind) => {
    if (!pad) { if (b === 'b') stop(null); return true; }
    const n = PAD_NAMES[b];
    if (n) stop(n[kind === 'ps' ? 1 : 0]);
    return true;
  };
  BIND = { stop };
}
// Which other actions a key is on too: said, not changed.
function bindDupes(o) {
  const by = {};
  for (const a of o.bindings.actions) for (const k of a.keys) (by[k.toLowerCase()] = by[k.toLowerCase()] || []).push(a.label);
  return (a) => [...new Set(a.keys.flatMap((k) => (by[k.toLowerCase()] || []).filter((l) => l !== a.label)))];
}
function bindRows(id, o) {
  const b = o.bindings, cap = S.bindCap && S.bindCap.id === id && S.bindCap.k === o.id ? S.bindCap : null;
  const dupes = bindDupes(o), off = o.applies ? '' : ' disabled';
  const waiting = (a) => esc(a.pad ? t('Press a key or button…') : t('Press a key…'));
  const rows = b.actions.map((a, ai) => {
    const at = (i) => cap && cap.ai === ai && cap.i === i;
    const chips = a.keys.map((k, i) => `<span class="kchip${at(i) ? ' cap' : ''}"><button type="button" class="kc" data-act="bindcap" data-id="${esc(id)}" data-k="${esc(o.id)}" data-a="${ai}" data-i="${i}"${off}>${at(i) ? waiting(a) : esc(k)}</button><button type="button" class="kx" data-act="bindclear" data-id="${esc(id)}" data-k="${esc(o.id)}" data-a="${ai}" data-i="${i}" aria-label="${esc(tf('Clear {key}', { key: k }))}"${off}>${XMARK}</button></span>`);
    if (a.keys.length < b.max) chips.push(`<button type="button" class="kc kadd${at(a.keys.length) ? ' cap' : ''}" data-act="bindcap" data-id="${esc(id)}" data-k="${esc(o.id)}" data-a="${ai}" data-i="${a.keys.length}" aria-label="${esc(tf('Add a key for {action}', { action: a.label }))}"${off}>${at(a.keys.length) ? waiting(a) : '+'}</button>`);
    const d = dupes(a);
    return `<div class="set-row kb-row"><div class="k"><b>${esc(a.label)}</b>${d.length ? `<span class="kdup">${esc(tf('Also used by {action}', { action: d.join(', ') }))}</span>` : ''}</div><div class="kchips">${chips.join('')}</div></div>`;
  }).join('');
  return `<div class="kb${o.applies ? '' : ' off'}"><div class="set-row kb-head"><div class="k"><b>${esc(o.label)}</b><span>${esc(o.help)}</span></div><button type="button" class="btn" data-act="bindreset" data-id="${esc(id)}" data-k="${esc(o.id)}"${b.changed && o.applies ? '' : ' disabled'}>${esc(t('Reset to Default'))}</button></div>${rows}</div>`;
}
function bindingOf(id, k) {
  const su = S.setup[id];
  return su && su !== 'loading' ? (su.settings || []).find((o) => o.id === k && o.bindings) : null;
}
function redrawSetup(id) {
  if (S.view === 'game' && S.arg === id && $('#g-body')) showTab(S.tab); else render(true);
  if (window.tvRefresh) window.tvRefresh();
}
async function bindSet(id, k, ai, keys) {
  const o = bindingOf(id, k), a = o && o.bindings.actions[ai];
  if (!a) return;
  try { a.keys = await invoke('set_game_binding', { id, setting: k, action: a.key, keys }); } catch (e) { fail(e); }
  delete S.setup[id];
  await loadSetup(id);
}
// One action's key i (a new one when i is past the last): the next key pressed.
function bindAsk(id, k, ai, i, onWait) {
  const o = bindingOf(id, k), a = o && o.bindings.actions[ai];
  if (!a || !o.applies) return;
  S.bindCap = { id, k, ai, i };
  if (onWait) onWait(a); else redrawSetup(id);
  bindCapture(a.pad, (name) => {
    S.bindCap = null;
    if (onWait) onWait(null);
    if (!name) return redrawSetup(id);
    const keys = [...a.keys];
    keys[Math.min(i, keys.length)] = name;
    bindSet(id, k, ai, keys);
  });
}
window.bindAsk = bindAsk;
window.bindSet = bindSet;
window.bindDupes = bindDupes;
// A slider moved: written at once (by drag, on release; by keys, a moment after the last press).
const stepPlaces = (st) => { const d = String(st).split('.')[1]; return d ? Math.min(10, d.length) : 0; };
function sliderOf(el) {
  const su = S.setup[el.dataset.id];
  return su && su !== 'loading' ? (su.settings || []).find((o) => o.id === el.dataset.k && o.slider) : null;
}
const slideWrites = {};
function slideTo(el, v, later) {
  const o = sliderOf(el);
  if (!o) return;
  const sl = o.slider, out = el.parentNode.querySelector('output');
  v = Math.min(sl.max, Math.max(sl.min, v));
  const shown = v.toFixed(Math.max(stepPlaces(sl.step), stepPlaces(sl.min)));
  el.value = v;
  if (out) out.textContent = shown;
  const key = el.dataset.id + '|' + el.dataset.k;
  clearTimeout(slideWrites[key]);
  const write = async () => {
    try { const txt = await invoke('set_game_slider', { id: el.dataset.id, setting: el.dataset.k, value: v }); sl.value = txt; if (out) out.textContent = txt; } catch (e) { fail(e); }
  };
  if (later) slideWrites[key] = setTimeout(write, 300); else write();
}
document.addEventListener('input', (e) => {
  const el = e.target.closest && e.target.closest('input[data-gslide]');
  if (!el) return;
  const o = sliderOf(el), out = el.parentNode.querySelector('output');
  if (o && out) out.textContent = Number(el.value).toFixed(Math.max(stepPlaces(o.slider.step), stepPlaces(o.slider.min)));
});
document.addEventListener('change', (e) => {
  const el = e.target.closest && e.target.closest('input[data-gslide]');
  if (el) slideTo(el, Number(el.value));
});
// Left and right: one step from the value it really has (one set in the game, off the step,
// goes to the next step up or down).
document.addEventListener('keydown', (e) => {
  const el = e.target.closest && e.target.closest('input[data-gslide]');
  if (!el || !['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown'].includes(e.key)) return;
  const o = sliderOf(el);
  if (!o || el.disabled) return;
  e.preventDefault();
  e.stopPropagation();
  const sl = o.slider, cur = parseFloat((el.parentNode.querySelector('output') || {}).textContent) || parseFloat(sl.value) || sl.min;
  const up = e.key === 'ArrowRight' || e.key === 'ArrowUp', k = (cur - sl.min) / sl.step;
  const n = up ? Math.floor(k + 1e-9) + 1 : Math.ceil(k - 1e-9) - 1;
  const m = 10 ** Math.max(stepPlaces(sl.step), stepPlaces(sl.min));
  slideTo(el, Math.round((sl.min + n * sl.step) * m) / m, true);
}, true);
async function setGameSetting(id, k, i) {
  try { await invoke('set_game_setting', { id, setting: k, choice: Number(i) }); profileSaid(id, 'settings', true); } catch (e) { profileSaid(id, 'settings', false, e); return fail(e); }
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
  if (Looks.failed()) return `<div class="stack"><div class="card"><h3>${esc(t('Shaders didn\'t load'))}</h3><p class="muted" style="margin:8px 0 0">${esc(t('Restart the launcher to try again.'))}</p></div></div>`;
  if (!li || li === 'loading' || !Looks.defs()) return `<div class="stack"><div class="card"><p class="muted" style="margin:0">${esc(t('Loading…'))}</p></div></div>`;
  if (!li.ok) return `<div class="stack"><div class="card"><h3>${esc(t('Shaders aren\'t available for this game'))}</h3><p class="muted" style="margin:8px 0 0">${esc(t(li.why))}</p></div></div>`;
  const D = Looks.defs(), cur = li.look || '', look = D.looks.find((l) => l.id === cur);
  const all = [LOOK_NONE].concat(D.looks);
  const crt = look && look.lines;
  const origin = originName(a.origin);
  const src = (li.shot ? t('Your latest screenshot.') : t('The game\'s banner. Take a screenshot while you play (F12) to see shaders on the game itself.')) + (look ? ' ' + t('Drag across the picture to compare.') : '');
  const amountRow = `<div class="lk-amtrow${look ? '' : ' off'}"><div class="k"><b>${esc(t('Amount'))}</b><span>${esc(look ? tf('How strong {look} is.', { look: look.name === 'CRT' ? t('the CRT shader') : t(look.name) === look.name ? look.name.toLowerCase() : t(look.name) }) : t('Pick a shader first.'))}</span></div>
        <div class="lk-amt"><span>${esc(t('Light'))}</span><input type="range" min="0" max="100" step="5" value="${li.amount}" data-lk-amount="${esc(id)}"${look ? '' : ' disabled'} aria-label="${esc(t('Amount'))}"><span>${esc(t('Strong'))}</span></div></div>`;
  // The picture and its strength on one side, every shader in a short list beside it: one look
  // at the tab shows them all, and picking one never scrolls the picture away.
  return `<div class="stack lk" data-lk="${esc(id)}">
    <div class="lk-studio">
    <div class="lk-main">
    <div class="lk-view${look ? '' : ' none'}" style="--x:50%">
      <canvas class="lk-after" aria-hidden="true"></canvas>
      <canvas class="lk-before" aria-hidden="true"></canvas>
      <div class="lk-split" aria-hidden="true"><i></i></div>
      <span class="lk-tag b">${esc(t('Original'))}</span><span class="lk-tag a">${esc(look ? t(look.name) : t('Original'))}</span>
      <input class="lk-range" type="range" min="0" max="100" value="50" aria-label="${esc(t('Compare with the original'))}" data-lk-split>
      <p class="lk-nopic">${esc(t('No picture to show it on yet. Take a screenshot while you play (F12).'))}</p>
    </div>
    <p class="lk-src">${esc(src)}</p>
    ${amountRow}
    </div>
    <div class="lk-list" role="radiogroup" aria-label="${esc(t('Shader'))}">${all.map((l) => `<button type="button" role="radio" class="lk-item" aria-checked="${l.id === cur}" data-act="look" data-id="${esc(id)}" data-look="${esc(l.id)}"><span class="lk-th"><canvas data-look-th="${esc(l.id)}"></canvas></span><span class="lk-txt"><b>${esc(t(l.name))}</b><span>${esc(t(l.blurb))}</span></span></button>`).join('')}</div>
    </div>
    <div class="card">
      ${crt ? `<div class="set-row"><div class="k"><b>${esc(t('Scanlines'))}</b><span>${esc(origin ? tf('{n} lines, like the {console} sent to a TV.', { n: li.lines, console: origin }) : tf('{n} lines, like the console sent to a TV.', { n: li.lines }))}</span></div></div>` : ''}
      ${S.os === 'linux' ? `<div class="set-row"><div class="k"><b>${esc(t('In the game'))}</b><span>${esc(t('Press Home while you play to turn the shader off and on again.'))}</span></div></div>
      <div class="set-row"><div class="k"><b>vkBasalt</b><span>${esc(t('On Linux, shaders run on vkBasalt, from your distro\'s packages. The launcher turns it on only for the game you start.'))}</span></div><button type="button" class="btn sm" data-url="https://github.com/DadSchoorse/vkBasalt">GitHub</button></div>` : `<div class="set-row"><div class="k"><b>${esc(t('In the game'))}</b><span>${esc(t('Press Home while you play to fine-tune it in ReShade\'s menu. Changes made there stay until you pick another shader here.'))}</span></div></div>
      <div class="set-row"><div class="k"><b>ReShade${li.reshade ? ' ' + esc(li.reshade) : ''}</b><span>${esc(t('Shaders run on ReShade by crosire.'))} ${esc(li.reshade ? t('The launcher keeps it up to date.') : t('It downloads the first time you play with a shader, about 5 MB.'))} ${esc(t('Nothing is installed into Windows.'))}</span></div><button type="button" class="btn sm" data-url="https://reshade.me">reshade.me</button></div>`}
    </div></div>`;
}
// The picture looks are shown on: the player's screenshot, or the game's banner.
// A decoded one is about 3 MB, so only the few most recently shown are kept. The encoded picture
// stays in the disk/HTTP cache, so one that was let go decodes again quickly.
const LOOK_PIC_MAX = 3;
const LOOK_PIC = {};
function lookPicture(id) {
  if (LOOK_PIC[id]) {
    // Using one makes it the most recent, so the ones not used lately go first.
    const pic = LOOK_PIC[id];
    delete LOOK_PIC[id];
    LOOK_PIC[id] = pic;
    return pic;
  }
  const li = S.looks[id] || {}, a = byId(id) || {};
  const pic = LOOK_PIC[id] = (async () => {
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
  pic.then((img) => { if (!img && LOOK_PIC[id] === pic) delete LOOK_PIC[id]; });
  const keys = Object.keys(LOOK_PIC);
  for (let i = 0; i < keys.length - LOOK_PIC_MAX; i++) if (keys[i] !== id) delete LOOK_PIC[keys[i]];
  return pic;
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
  if (el.matches('[data-act="dl-limit"]')) { saveUi({ dl_limit: Number(el.value) || 0 }).then(sendLimits); return; }
  if (el.matches('[data-act="proxy-mode"]')) { if (el.value === 'custom') { S.proxyMode = 'custom'; setSection('updates', false); } else saveProxy(el.value); return; }
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
  const q = fold(S.modQ.trim());
  const list = ml && ml.list ? ml.list.filter((p) => !q || fold([p.name, p.owner, p.desc, ...(p.categories || [])].join(' ')).includes(q)).slice(0, 60) : [];
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
  if (window.tvRedraw) window.tvRedraw();
}
const took = (s) => (s < 60 ? tf('{n} sec', { n: Math.max(1, s) }) : tf('{n} min', { n: Math.round(s / 60) }));
// (0.91) Each download's speed over its last minute and a half, for the graph at the top of Downloads.
// A short download barely drew a line with one point a second, so the speed is worked out here from
// the bytes each progress update brings (several a second), smoothed a little, and kept five times a
// second: a 3-second download draws a real curve.
const DLSPD = {};
const DL_STEP = 200, DL_KEEP = 450; // a point every 200 ms, a minute and a half of them
function noteSpeed(id, p) {
  const l = DLSPD[id] = DLSPD[id] || [], now = Date.now(), done = p.done || 0;
  const m = l.m || (l.m = { at: now, done, v: p.speed || 0 });
  const dt = now - m.at;
  if (done < m.done) { m.at = now; m.done = done; } // started over (a retry): measure from here
  else if (dt >= 120) {
    const got = (done - m.done) / (dt / 1000);
    m.v = m.v ? m.v * 0.55 + got * 0.45 : got;
    m.at = now;
    m.done = done;
  }
  const v = Math.round(m.v || p.speed || 0), slot = Math.floor(now / DL_STEP);
  if (l.length && slot === l.slot) l[l.length - 1] = v; else { l.push(v); l.slot = slot; }
  if (l.length > DL_KEEP) l.splice(0, l.length - DL_KEEP);
}
window.dlSpeedSeed = (id, list) => { DLSPD[id] = list.slice(-DL_KEEP); DLSPD[id].slot = Math.floor(Date.now() / DL_STEP); };
function dlGraph(id) {
  // Fills the width from the start, then scrolls once there's a minute and a half of it.
  const l = DLSPD[id] || [], W = 300, H = 96, n = Math.max(l.length, 2);
  const top = Math.max(1, ...l) * 1.15;
  const pts = l.map((v, i) => [((n - l.length + i) / (n - 1)) * W, H - (v / top) * (H - 6)]);
  const line = pts.map((p, i) => (i ? 'L' : 'M') + p[0].toFixed(1) + ' ' + p[1].toFixed(1)).join(' ');
  const area = pts.length > 1 ? `${line} L${W} ${H} L${pts[0][0].toFixed(1)} ${H} Z` : '';
  return `<defs><linearGradient id="dlg" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#34F2E8" stop-opacity=".42"/><stop offset="1" stop-color="#34F2E8" stop-opacity="0"/></linearGradient></defs>
    ${[0.25, 0.5, 0.75].map((f) => `<line x1="0" x2="${W}" y1="${H * f}" y2="${H * f}" class="gl"/>`).join('')}${area ? `<path d="${area}" fill="url(#dlg)"/><path d="${line}" class="ln"/>` : ''}`;
}
function dlGraphStats(id) {
  const j = S.jobs[id] || {}, l = DLSPD[id] || [];
  const cur = j.phase === 'download' ? j.speed || 0 : 0, peak = Math.max(0, ...l);
  const left = cur && j.total ? Math.max(0, j.total - j.done) / cur : 0;
  return [[t('Current'), cur ? tf('{speed}/s', { speed: fmtB(cur) }) : '—'], [t('Peak'), peak ? tf('{speed}/s', { speed: fmtB(peak) }) : '—'],
    [t('Time Left'), left ? took(Math.round(left)) : '—']];
}
// Only the graph and its numbers change while a download runs.
function paintDlHero(id) {
  const g = document.querySelector(`[data-dl-graph="${CSS.escape(id)}"]`);
  if (!g) return;
  g.innerHTML = dlGraph(id);
  const v = dlGraphStats(id);
  document.querySelectorAll(`[data-dl-gstat="${CSS.escape(id)}"] b`).forEach((b, i) => { b.textContent = v[i][1]; });
  const ph = document.querySelector(`[data-dl-phase="${CSS.escape(id)}"]`);
  if (ph) ph.textContent = dlPhase(id);
}
function dlPhase(id) {
  const j = S.jobs[id] || (S.paused || {})[id] || {};
  if (!S.jobs[id]) return t('Paused');
  return j.phase === 'download' && j.total ? tf('Downloading · {pct}%', { pct: pct(j) }) : progLabel(j)[0] || t('Getting ready');
}
// (0.91) How the store checked a finished download's files (ClamAV or VirusTotal), asked once each.
S.scans = S.scans || {};
const scanAsk = new Set();
// What the store's check of a finished download says: { cls, ic, text, scanner, sha }, or null
// while it isn't known (asked once).
function scanInfo(h) {
  const key = h.id + '@' + h.tag, files = S.scans[key];
  if (files === undefined && !scanAsk.has(key) && h.tag) {
    scanAsk.add(key);
    invoke('scan_info', { id: h.id, tag: h.tag }).then((r) => (r && r.files) || []).catch(() => []).then((f) => {
      S.scans[key] = f;
      for (const el of document.querySelectorAll(`[data-scan="${CSS.escape(key)}"]`)) el.outerHTML = scanPill(h);
      if (document.body.classList.contains('tv-on') && window.tvRedraw) window.tvRedraw();
    });
  }
  const mine = (files || []).filter((f) => (f.for || []).includes(S.os));
  const use = mine.length ? mine : files || [];
  if (!use.length) return null;
  const rank = { red: 3, yellow: 2, grey: 1, green: 0 };
  const worst = use.reduce((w, f) => (rank[f.verdict] > rank[w.verdict] ? f : w), use[0]);
  const by = worst.scanner === 'clamav' ? 'ClamAV' : 'VirusTotal';
  const [cls, ic, text] = worst.verdict === 'green' ? ['ok', '✓', tf('Clean · {by}', { by })]
    : worst.verdict === 'grey' ? ['wait', '…', t('Being checked')]
    : worst.verdict === 'yellow' ? ['warn', '!', worst.scanner === 'clamav' ? tf('Flagged · {by}', { by }) : tf('{n} of {total} engines flagged', { n: worst.found, total: worst.engines })]
    : ['bad', '✕', tf('Flagged · {by}', { by })];
  return { cls, ic, text, scanner: worst.scanner || '', sha: scanSha(worst) };
}
function scanPill(h) {
  const key = h.id + '@' + h.tag, info = scanInfo(h);
  if (!info) return h.verified ? `<span class="dl-check" data-scan="${esc(key)}">${esc(t('Checksum matched'))}</span>` : `<span data-scan="${esc(key)}"></span>`;
  // It isn't files that were flagged: it's the engines (VirusTotal) or ClamAV that checked the
  // download. The pill opens VirusTotal's report for that file, or says how ClamAV checked it.
  return `<button type="button" class="scan-pill ${info.cls}" data-scan="${esc(key)}" data-act="scan-open" data-scanner="${esc(info.scanner)}" data-sha="${esc(info.sha)}" title="${esc(t('Every release is scanned before it\'s published. A few flags on a new recomp are usually false positives.'))}"><i>${info.ic}</i>${esc(info.text)}</button>`;
}
// The file's sha256, from the scan or the catalog's files[] entry for that release.
function scanSha(f) {
  const v = String(f.sha256 || f.sha || f.hash || '').replace(/^sha256:/i, '').toLowerCase();
  return /^[0-9a-f]{64}$/.test(v) ? v : '';
}
const VT_URL = (sha) => `https://www.virustotal.com/gui/file/${sha}`;
function scanOpen(el) {
  document.querySelectorAll('.scan-pop').forEach((x) => x.remove());
  if (el.dataset.scanner !== 'clamav' && el.dataset.sha) return invoke('open_url', { url: VT_URL(el.dataset.sha) }).catch(fail);
  const pop = document.createElement('div');
  pop.className = 'scan-pop';
  pop.setAttribute('role', 'status');
  pop.textContent = el.dataset.scanner === 'clamav' ? t('Checked by ClamAV on recompstore.com before it was published.') : t('Every release is scanned before it\'s published. A few flags on a new recomp are usually false positives.');
  const r = el.getBoundingClientRect();
  pop.style.left = Math.max(12, Math.min(innerWidth - 300, r.left)) + 'px';
  pop.style.top = (r.bottom + 8) + 'px';
  document.body.append(pop);
  setTimeout(() => document.addEventListener('pointerdown', () => pop.remove(), { once: true }), 0);
  setTimeout(() => pop.remove(), 6000);
}
function pageDownloads() {
  if (!S.dlHist) loadDlHist();
  const hist = S.dlHist || [];
  const act = Object.keys(S.jobs).concat(Object.keys(S.paused || {}).filter((id) => !S.jobs[id])), fails = S.recent.filter((r) => !S.jobs[r.id] && r.phase === 'error');
  const ups = Object.keys(S.updates).filter((id) => S.library[id] && !S.jobs[id] && !(S.paused || {})[id]);
  const appOf = (id) => { const pj = S.jobs[id] && S.jobs[id].plug ? S.jobs[id] : null; return pj ? { id, name: pj.name, plug: pj } : byId(id) || { id, name: S.library[id]?.name || id }; };
  // A row's picture: the game's wide art when it has some (like Steam's), else its cover.
  const wide = (id, w = 320) => { const a = appOf(id); if (a.plug) { const pm = plugMan(a.plug.plugin); return `<span class="cap ph plug-ph">${pm ? plugIcon(pm, pm.icon) : PUZZLE}</span>`; }
    return a.hero ? pic(thumbOf(a.hero, w), 'cap') : a.cover ? `<span class="cap cov">${pic(thumbOf(a.cover, 160))}</span>` : '<span class="cap ph"></span>'; };
  const total = hist.reduce((n, h) => n + (h.size || 0), 0), secs = hist.reduce((n, h) => n + (h.secs || 0), 0);
  const avg = secs ? total / secs : 0;
  const nInst = libInstalls().filter((g) => !g.local).length;
  const disk = libInstalls().reduce((n, g) => n + (g.size || 0), 0);
  const stats = [
    [t('Downloaded'), fmtB(total) || '0 MB', tn(hist.length, '1 download', '{n} downloads')],
    [t('In your library'), tn(nInst, '1 game', '{n} games'), fmtB(disk) ? tf('{size} downloaded', { size: fmtB(disk) }) : ''],
    [t('Updates installed'), fmtNum(hist.filter((h) => h.update).length), ups.length ? tn(ups.length, '1 more ready', '{n} more ready') : t('All up to date')],
    [t('Average speed'), avg ? tf('{speed}/s', { speed: fmtB(avg) }) : '—', hist[0] ? tf('Last one {when}', { when: ago(hist[0].when) }) : ''],
  ];
  // The one at the top: the download that's moving, else the first one there is.
  const lead = act.find((id) => S.jobs[id] && S.jobs[id].phase !== 'queued') || act[0];
  const kindTag = (id) => { const j = S.jobs[id] || {}; return j.plug ? ((plugMan(j.plugin) || {}).name || t('Plugin')) : S.library[id] ? t('Update') : t('New'); };
  const jobBtns = (id, small) => { const j = S.jobs[id] || {}, held = !S.jobs[id], sm = small ? ' sm' : '';
    const two = held ? `<button type="button" class="btn main${sm}" data-act="dl-resume" data-id="${esc(id)}">${esc(t('Resume'))}</button>` : canPause(id) && j.phase !== 'queued' ? `<button type="button" class="btn${sm}" data-act="dl-pause" data-id="${esc(id)}">${esc(t('Pause'))}</button>` : '';
    return `${two}<button type="button" class="btn${sm}" data-act="cancel" data-id="${esc(id)}"${j.phase === 'unpack' ? ' disabled' : ''}>${esc(t('Cancel'))}</button>`; };
  const hero = lead ? (() => { const a = appOf(lead), art = a.hero || a.cover, gs = dlGraphStats(lead);
    return `<section class="dl-hero">
      <div class="dl-hero-art">${art && !a.plug ? pic(thumbOf(art, 640)) : wide(lead)}</div>
      <div class="dl-hero-main"><div class="dl-top"><span class="tag cy">${esc(kindTag(lead))}</span><span class="dl-phase" data-dl-phase="${esc(lead)}">${esc(dlPhase(lead))}</span></div>
        <h2>${esc(a.name)}</h2>${progress(lead)}<div class="acts">${jobBtns(lead)}</div></div>
      <div class="dl-graph"><svg viewBox="0 0 300 96" preserveAspectRatio="none" data-dl-graph="${esc(lead)}" aria-hidden="true">${dlGraph(lead)}</svg>
        <div class="dl-gstats" data-dl-gstat="${esc(lead)}">${gs.map(([k, v]) => `<div><span>${esc(k)}</span><b>${esc(v)}</b></div>`).join('')}</div></div></section>`; })() : '';
  // RecompLauncher's own update: up top, where it's seen first.
  const self = S.launcherUpd ? `<section class="dl-self"><img src="img/mark.svg" alt=""><div class="mid"><b>${esc(tf('RecompLauncher {version} is ready', { version: verName(S.launcherUpd.version) }))}</b><span>${esc(tf('You have {version}. It updates in a moment and opens again by itself.', { version: verName(S.version) }))}</span></div><button type="button" class="btn main" data-act="self-update">${DOWN}${esc(t('Update now'))}</button></section>` : '';
  const queue = act.filter((id) => id !== lead).map((id) => { const a = appOf(id);
    return `<div class="dl-row dl-q">${wide(id)}<div class="mid"><b>${esc(a.name)}</b>${progress(id)}</div><span class="tag cy">${esc(kindTag(id))}</span><span class="acts">${jobBtns(id, true)}</span></div>`; }).join('');
  const upRows = ups.map((id) => { const a = appOf(id);
    return `<div class="dl-row link" data-act="open" data-id="${esc(id)}">${wide(id)}<div class="mid"><b>${esc(a.name)}</b><span>${esc(tf('{from} → {to}', { from: S.library[id].tag || '?', to: S.updates[id] }))}</span></div><button type="button" class="btn main sm" data-act="install" data-id="${esc(id)}">${DOWN}${esc(t('Update'))}</button></div>`; }).join('');
  const failRows = fails.map((r) => { const a = appOf(r.id);
    return `<div class="dl-row bad">${wide(r.id)}<div class="mid"><b>${esc(a.name)}</b><span class="err">${esc(t(r.msg))}</span></div><button type="button" class="btn sm" data-act="install" data-id="${esc(r.id)}">${esc(t('Try again'))}</button></div>`; }).join('');
  const ui = S.settings.ui || {};
  const shown = hist.filter((h) => (h.when || 0) > (ui.dl_cleared || 0));
  const histRows = shown.slice(0, 40).map((h) => { const a = appOf(h.id); const inst = S.library[h.id];
    // The row opens the game's page; Play is its own button.
    return `<div class="dl-row${byId(h.id) || inst ? ' link" data-act="open" data-id="' + esc(h.id) : ''}">${wide(h.id)}<div class="mid"><b>${esc(a.name || h.name)}</b><span class="dl-sub"><span>${esc(h.update ? tf('Updated to {tag}', { tag: h.tag }) : tf('Installed {tag}', { tag: h.tag }))}</span>${scanPill(h)}</span></div>
      <span class="dl-meta"><b>${esc(fmtB(h.size))}</b><small>${esc(took(h.secs || 0))}</small></span><span class="dl-when">${esc(agoCap(h.when))}</span>
      ${inst ? `<button type="button" class="btn sm dl-play" data-act="play" data-id="${esc(h.id)}">${PLAY}${esc(t('Play'))}</button>` : '<span class="dl-play" aria-hidden="true"></span>'}</div>`; }).join('');
  const nothing = !act.length && !ups.length && !fails.length && !shown.length;
  const updLog = updateLog(hist).filter((u) => (u.when || 0) > (ui.upd_cleared || 0));
  const tab = S.dlTab === 'updates' ? 'updates' : 'downloads';
  const tabs = `<div class="tabs dl-tabs"><div class="tabs-in" role="tablist"><span class="tab-pill" aria-hidden="true"></span>${[['downloads', t('Downloads')], ['updates', t('History')]].map(([k, n]) => `<button type="button" role="tab" class="${tab === k ? 'on' : ''}" aria-selected="${tab === k}" data-act="dl-tab" data-tab="${k}">${esc(n)}</button>`).join('')}</div></div>`;
  // Clearing only hides what's listed; the totals above stay.
  const canClear = tab === 'updates' ? updLog.length : shown.length;
  const bar = `<div class="dl-bar">${tabs}${canClear ? `<button type="button" class="btn sm" data-act="dl-clear" data-k="${tab === 'updates' ? 'upd_cleared' : 'dl_cleared'}">${esc(t('Clear history'))}</button>` : ''}</div>`;
  const head = `<div class="head"><div><h1>${esc(t('Downloads'))}</h1><p>${esc(tab === 'updates' ? t('Every update the launcher has installed, newest first. Open one to read what changed.') : t('Each download is matched against GitHub\'s checksum, and the store checks releases for viruses before offering them.'))}</p></div></div>${self}${bar}`;
  if (tab === 'updates') return `<div class="page dl-page" style="max-width:1100px">${head}${updatesTab(updLog)}</div>`;
  const n = (k) => `<span class="dl-n">${k}</span>`;
  return `<div class="page dl-page" style="max-width:1100px">${head}
    ${hero || `<div class="stats dl-stats">${stats.map(([k, v, sub]) => `<div><span>${esc(k)}</span><b>${esc(v)}</b><small>${esc(sub || '')}</small></div>`).join('')}</div>`}
    ${queue || upRows ? `<section class="shelf"><div class="shelf-h"><h2>${esc(t('Up Next'))}${n(act.length - (lead ? 1 : 0) + ups.length)}</h2>${ups.length > 1 ? `<button type="button" class="btn sm" data-act="update-all">${DOWN}${esc(t('Update all'))}</button>` : ''}</div><div class="dl-list">${queue}${upRows}</div></section>` : ''}
    ${failRows ? `<section class="shelf"><div class="shelf-h"><h2>${esc(t('Didn\'t finish'))}${n(fails.length)}</h2></div><div class="dl-list">${failRows}</div></section>` : ''}
    ${shown.length ? `<section class="shelf"><div class="shelf-h"><h2>${esc(t('Completed'))}${n(shown.length)}</h2></div><div class="dl-list">${histRows}</div></section>` : ''}
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
    return n && relKey(n.version) === relKey(u.tag) && n.notes ? `<div class="md">${md(n.notes, true)}</div>` : `<p class="muted">${esc(t('The notes for this version are on recomplauncher.com.'))}</p>`;
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
    const img = u.self ? '<span class="upd-self cap" aria-hidden="true"><img src="img/mark.svg" alt=""></span>' : a.hero ? pic(thumbOf(a.hero, 320), 'cap') : a.cover ? `<span class="cap cov">${pic(thumbOf(a.cover, 160))}</span>` : '<span class="cap ph"></span>';
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
// Who looks after a game's setup (the catalog's maintained_by), and what Developer Tools calls the
// setup's flow and what it can do.
const OWNER_NAME = { project: N_('Its Developer'), official: N_('Its Developer'), launcher: N_('RecompLauncher'), community: N_('The Community') };
const FLOW_NAME = { launch: [N_('Launches With the Player\'s Copy'), N_('The game gets the player\'s disc or files when it starts. Nothing runs before Play.')], prepare: [N_('Needs a One-Time Step'), N_('A step runs once before Play (importing or recompiling from the player\'s copy).')], '': [N_('Plays as It Is'), N_('It doesn\'t ask the player for anything.')] };
const CAP_NAME = { settings: N_('Launcher Settings'), verify: N_('Known-Dump Check'), inputs: N_('More Than One File'), saves: N_('Says Where It Saves') };
const OWNER_LONG = { project: N_('Maintained by its developer'), launcher: N_('Maintained by RecompLauncher'), community: N_('Maintained by the community'), unconfirmed: N_('Nobody has confirmed who maintains it') };
const DEV_MARK = { ok: '<path d="m5 12.5 4.5 4.5L19 7.5"/>', warn: '<path d="M12 8v5M12 16.5v.01"/>', err: '<path d="m7 7 10 10M17 7 7 17"/>' };
function devTab(a) {
  const id = a.id, g = S.library[id], r = S.dev[id];
  if (r === undefined) { devCheck(id); return `<div class="stack"><div class="card"><p class="muted" style="margin:0">${esc(t('Checking…'))}</p></div></div>`; }
  if (r === 'loading') return `<div class="stack"><div class="card"><p class="muted" style="margin:0">${esc(t('Checking…'))}</p></div></div>`;
  const flow = FLOW_NAME[r.flow || ''] || FLOW_NAME[''];
  const src = { appimage: t('The manifest inside the AppImage'), folder: t('Your recomplauncher.json'), catalog: t('The file on recomplauncher.com'), 'built-in': t('The file built into the launcher'), none: t('No file yet') }[r.source] || '';
  const errs = r.items.filter((x) => x.level === 'err').length, warns = r.items.filter((x) => x.level === 'warn').length;
  return `<div class="stack">
    <div class="card dev-head"><div class="dev-sum"><span class="kick">${esc(t('Local build'))}</span><h3>${esc(src)}</h3>
      <p class="muted">${esc(r.error ? t('The file has a problem. Players would get none of it.') : errs ? tn(errs, '1 problem to fix.', '{n} problems to fix.') : warns ? tn(warns, 'Works, with 1 thing worth a look.', 'Works, with {n} things worth a look.') : t('Everything checks out.'))}</p></div>
      <div class="acts"><button type="button" class="btn main" data-act="dev-check" data-id="${esc(id)}">${esc(t('Check again'))}</button>${r.has_file ? `<button type="button" class="btn" data-act="dev-file" data-id="${esc(id)}">${esc(t('Open the folder'))}</button>` : `<button type="button" class="btn" data-act="dev-starter" data-id="${esc(id)}">${esc(t('Make a starter file'))}</button>`}</div></div>
    <div class="dev-sum2"><div class="card"><span class="kick">${esc(t('Setup'))}</span><b>${esc(t(flow[0]))}</b><p class="muted">${esc(t(flow[1]))}</p></div>
      <div class="card"><span class="kick">${esc(t('What It Can Do'))}</span>${(r.caps || []).length ? `<div class="dev-caps">${r.caps.map((c) => `<span class="dev-cap">${esc(t(CAP_NAME[c] || c))}</span>`).join('')}</div>` : `<p class="muted">${esc(t('Nothing more yet.'))}</p>`}<p class="muted">${esc(t(OWNER_LONG[r.owner] || OWNER_LONG.unconfirmed))}</p></div></div>
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
  bring: '<path d="M12 3v11M7.5 9.5 12 14l4.5-4.5"/><path d="M4 14v4.5A1.5 1.5 0 0 0 5.5 20h13a1.5 1.5 0 0 0 1.5-1.5V14"/>',
  look: '<circle cx="12" cy="12" r="8.5"/><path d="M12 3.5v17M12 3.5a8.5 8.5 0 0 1 0 17" fill="currentColor"/>',
  bigpicture: '<rect x="2.5" y="4.5" width="19" height="13" rx="2.5"/><path d="M8 21h8"/><path d="m10.5 8.5 4 2.5-4 2.5z" fill="currentColor"/>',
  pads: '<path d="M7 9h10a4 4 0 0 1 3.9 4.9l-.8 3.4a2 2 0 0 1-3.5.8L15 16H9l-1.6 2.1a2 2 0 0 1-3.5-.8l-.8-3.4A4 4 0 0 1 7 9z"/><path d="M8.5 11.5v3M7 13h3"/>',
  pc: '<rect x="3" y="4" width="18" height="12" rx="2"/><path d="M8 20h8M12 16v4"/>',
  dev: '<path d="m8 8-4 4 4 4M16 8l4 4-4 4M13.5 5l-3 14"/>',
  about: '<circle cx="12" cy="12" r="9"/><path d="M12 11v6M12 7.5v.01"/>',
  plugins: '<path d="M10 4.5a2 2 0 0 1 4 0V6h4a1 1 0 0 1 1 1v4h-1.5a2 2 0 0 0 0 4H19v4a1 1 0 0 1-1 1h-4v-1.5a2 2 0 0 0-4 0V20H6a1 1 0 0 1-1-1v-4h1.5a2 2 0 0 0 0-4H5V7a1 1 0 0 1 1-1h4z"/>',
  beta: '<path d="M9.5 3h5M10 3v6.2L4.8 18.1A2 2 0 0 0 6.5 21h11a2 2 0 0 0 1.7-2.9L14 9.2V3"/><path d="M7.2 14h9.6"/>',
};
// Settings → Network: the proxy every request goes through (the list, GitHub, downloads, the
// website). Off, the system's (HTTPS_PROXY / ALL_PROXY), or the player's own address. The password
// is sealed by the core and never comes back here.
function networkGroup(group, row, st) {
  const mode = S.proxyMode || st.proxy || 'system';
  const custom = mode === 'custom';
  const pick = `<select class="sel" data-act="proxy-mode" aria-label="${esc(t('Proxy'))}">${[['off', t('Off')], ['system', t('Use the System Proxy')], ['custom', t('Custom')]].map(([k, n]) => `<option value="${k}"${mode === k ? ' selected' : ''}>${esc(n)}</option>`).join('')}</select>`;
  const fields = custom ? `<div class="set-row proxy-row"><div class="k"><b>${esc(t('Proxy Address'))}</b><span>${esc(t('http, https or socks5, like http://127.0.0.1:7890 or socks5://host:1080. User and password are optional.'))}</span></div>
    <span class="acts proxy-fields"><input class="field sm" id="proxy-url" type="text" autocomplete="off" spellcheck="false" placeholder="http://127.0.0.1:7890" value="${esc(st.proxy_url || '')}" aria-label="${esc(t('Proxy Address'))}"><input class="field sm" id="proxy-user" type="text" autocomplete="off" spellcheck="false" placeholder="${esc(t('User'))}" value="${esc(st.proxy_user || '')}" aria-label="${esc(t('User'))}"><input class="field sm" id="proxy-pass" type="password" autocomplete="off" placeholder="${esc(st.proxy_pass ? '••••••' : t('Password'))}" aria-label="${esc(t('Password'))}"><button type="button" class="btn sm main" data-act="proxy-save">${esc(t('Save'))}</button></span></div>` : '';
  return group(t('Network'), row(t('Proxy'), t('Where the launcher\'s requests go: the list, GitHub, downloads and the website. Use one if GitHub can\'t be reached from your network.'), pick) + fields);
}
// The shared NVIDIA DLSS file, when the launcher keeps one: removing it affects every game that uses it.
function dlssGroup(group, row) {
  if (S.dlssState === undefined) { S.dlssState = null; invoke('dlss_state').then((v) => { S.dlssState = v; if (S.view === 'settings' && S.setTab === 'library') setSection('library', false); }).catch(() => {}); }
  if (!S.dlssState || !S.dlssState.have) return '';
  return group(t('NVIDIA DLSS'), row(t('Remove NVIDIA DLSS File'), t('Every game that uses it plays without DLSS until it\'s downloaded again, which happens when one next needs it. A DLSS file a game ships itself is never touched.'), `<button type="button" class="btn sm danger" data-act="dlss-remove">${esc(t('Remove'))}</button>`));
}
async function saveProxy(mode) {
  const url = ($('#proxy-url') || {}).value || '', user = ($('#proxy-user') || {}).value || '', passEl = $('#proxy-pass');
  const pass = passEl && passEl.value ? passEl.value : (user ? null : '');
  try {
    const r = await invoke('set_proxy', { mode, url, user, pass });
    Object.assign(S.settings, { proxy: mode, proxy_url: url.trim(), proxy_user: user, proxy_pass: r.has_pass ? 'sealed' : '' });
    S.proxyMode = null;
    toast(mode === 'off' ? t('Proxy off.') : mode === 'system' ? (r.shown ? tf('Using the system proxy, {proxy}.', { proxy: r.shown }) : t('Using the system proxy. None is set right now.')) : tf('Using the proxy {proxy}.', { proxy: r.shown || url }));
    if (S.view === 'settings') setSection('updates', false);
  } catch (e) { fail(e); }
}
const SET_SECTIONS = [['general', N_('General')], ['report', N_('Report a Problem')], ['beta', N_('Beta')], ['account', N_('Account')], ['library', N_('Games and folders')], ['bring', N_('Bring Your Games')], ['updates', N_('Updates and downloads')], ['playing', N_('While Playing')], ['look', N_('Appearance')], ['bigpicture', N_('Big Picture mode')], ['plugins', N_('Plugins')], ['pads', N_('Controllers')], ['pc', N_('Your PC')], ['dev', N_('Developers')], ['about', N_('About')]];
// Microsoft's Visual C++ runtime, installed by the launcher (its installer's own small window
// shows the progress, and Windows may ask for permission).
let vcBusy = false;
async function installVcredist() {
  if (vcBusy) return;
  vcBusy = true;
  toast(t('Getting the Visual C++ runtime from Microsoft…'));
  try {
    const r = await invoke('install_vcredist');
    toast(r === 'restart' ? t('The Visual C++ runtime is installed. Restart your PC to finish, then play again.') : r === 'already' ? t('The Visual C++ runtime is already on this PC, so it\'s missing something else. Troubleshoot on its page says what.') : t('The Visual C++ runtime is installed. Play again.'));
  } catch (e) { fail(e); }
  vcBusy = false;
}
// GE-Proton on Linux: what Windows recomps start through now, and the newest GE-Proton in one press.
function geRow(row) {
  if (S.os !== 'linux') return '';
  if (!S.protonNow && !S.protonAsked) { S.protonAsked = true; invoke('proton_now').then((r) => { S.protonNow = r; if (S.view === 'settings') setSection(S.setTab); }).catch(() => {}); }
  const now = S.protonNow || {}, busy = S.geBusy;
  const say = !S.protonNow ? t('Checking what this PC has…')
    : now.ge ? tf('Windows recomps play through {name}. Check for a newer one now and then.', { name: now.runner })
    : now.runner ? tf('Windows recomps play through {name} now. GE-Proton runs more of them, and it\'s free.', { name: now.runner })
    : t('Nothing on this PC can run Windows recomps yet. GE-Proton is free and takes a minute.');
  const label = busy ? (busy.phase === 'download' && busy.total ? tf('Downloading {pct}%', { pct: Math.floor(busy.done / busy.total * 100) }) : t('Unpacking…')) : now.ge ? t('Check for a Newer One') : t('Install GE-Proton');
  return row(t('GE-Proton'), say, `<button type="button" class="btn sm" data-act="ge-proton"${busy ? ' disabled' : ''}>${esc(label)}</button>`)
    // umu-run's Steam runtime half downloaded (a checksum failure, parts missing): Windows recomps
    // won't start through it until it's downloaded again.
    + (now.umu_broken ? row(t('Steam Runtime for umu-run'), t('It didn\'t finish downloading, so Windows recomps can\'t start through umu-run. Repair it and it downloads again when a game next starts.'), `<button type="button" class="btn sm main" data-act="umu-repair">${esc(t('Repair Runtime'))}</button>`) : '');
}
async function geInstall() {
  if (S.geBusy) return;
  S.geBusy = { phase: 'download', done: 0, total: 0 };
  if (S.view === 'settings') setSection(S.setTab);
  const was = (S.protonNow || {}).runner;
  try {
    const tag = await invoke('ge_proton_install');
    S.protonNow = await invoke('proton_now').catch(() => S.protonNow);
    toast(tag === was ? tf('{name} is the newest. You\'re all set.', { name: tag }) : tf('{name} is installed. Windows recomps play through it now.', { name: tag }));
  } catch (e) { fail(e); }
  S.geBusy = null;
  if (S.view === 'settings') setSection(S.setTab);
}
T.event.listen('ge_progress', ({ payload: p }) => {
  if (!S.geBusy) return;
  const pct = (x) => (x.total ? Math.floor(x.done / x.total * 100) : -1);
  const moved = pct(p) !== pct(S.geBusy) || p.phase !== S.geBusy.phase;
  S.geBusy = p;
  if (moved && S.view === 'settings') { const b = document.querySelector('[data-act="ge-proton"]'); if (b) b.textContent = p.phase === 'download' && p.total ? tf('Downloading {pct}%', { pct: pct(p) }) : t('Unpacking…'); }
});
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
          + row(t('Hide AI-assisted recomps'), t('Recomps whose developers used AI tools for code, art, writing or voice won\'t show in Discover.'), sw('hide_ai', st.hide_ai, t('Hide AI-assisted recomps')))
          + row(t('Show the AI-assisted tag'), t('The tag on game pages. What the developer used AI for is always in the game\'s Overview.'), sw('show_ai_tag', ui.show_ai_tag, t('Show the AI-assisted tag'), 'ui-toggle'))
          + row(t('Hidden games'), (ui.hidden || []).length ? t('Games you hid from Discover and Home. Right-click a game to hide it.') : t('Right-click a game in Discover and pick Hide from Discover to keep it out of the way.'), `<button type="button" class="btn" data-act="hidden-list"${(ui.hidden || []).length ? '' : ' disabled'}>${esc(t('Show Hidden Games'))}</button>`))
        + (S.os === 'windows' ? group(t('Portable'), portableRow(row)) : S.os === 'linux' ? group(t('Windows recomps'), row(t('Play Windows recomps with Proton'), t('Recomps that only have a Windows version install that one and play through Proton. Steam\'s Proton, GE-Proton, umu-launcher and Wine are found on their own.'), sw('proton', st.proton, t('Play Windows recomps with Proton'))) + geRow(row)) : '')
        + group(t('Notifications and sharing'), row(t('Notifications'), S.os === 'windows' ? t('A Windows notification when a new recomp comes out for a game you own.') : t('A notification when a new recomp comes out for a game you own.'), sw('notify', st.notify, t('Notifications')))
          + row(t('Discord status'), t('Shows the game you\'re playing on your Discord profile, with its cover and play time.'), sw('discord', st.discord !== false, t('Discord status')))
          + row(t('Share usage and diagnostics'), t('Your PC\'s parts, which games you play and for how long, and what went wrong when one crashes or won\'t install. It helps fix problems faster. No names, no files.'), `<span class="set-ctl2"><a href="#" data-act="url" data-url="https://recompstore.com/privacy#launcher">${esc(t('What\'s sent'))}</a>${sw('share_usage', st.data_asked && st.share_usage, t('Share usage and diagnostics'))}</span>`)
          + row(t('Share anonymous usage stats'), t('When the launcher is open and which recomps are being played, under a random ID that isn\'t linked to you or your account.'), sw('share_stats', st.data_asked && st.share_stats, t('Share anonymous usage stats')))
          + row(t('Your IDs'), S.ids ? tf('Diagnostics {a} · Stats {b}. Quote them to have what was sent removed. A reset starts new ones.', { a: S.ids.usage.slice(0, 8), b: S.ids.stats.slice(0, 8) }) : t('Quote them to have what was sent removed. A reset starts new ones.'), `<span class="set-ctl2"><button type="button" class="btn" data-act="copy-ids">${esc(t('Copy'))}</button><button type="button" class="btn" data-act="reset-ids">${esc(t('Reset'))}</button></span>`))
        + group(t('When you close the window'), row(t('Run in background'), t('Keep RecompLauncher running when you close the window. Turn off to exit on close. Quit always exits.'), sw('close_to_tray', st.close_to_tray, t('Run in background'))));
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
        ${libs.map((l) => `<div class="set-row"><div class="k"><b class="path">${esc(l.path)}</b><span>${l.free != null ? esc(tf('{free} free', { free: fmtB(l.free) })) + ' · ' : ''}${esc(tn(l.games, '1 game', '{n} games'))}${l.default ? ' · ' + esc(t('default')) : ''}</span></div><div class="acts"><button type="button" class="btn sm" data-act="folder" data-path="${esc(l.path)}">${esc(t('Open'))}</button>${l.default ? '' : `<button type="button" class="btn sm" data-act="lib-default" data-path="${esc(l.path)}">${esc(t('Make default'))}</button><button type="button" class="btn sm danger" data-act="lib-remove" data-path="${esc(l.path)}" data-n="${l.games || 0}">${esc(t('Remove'))}</button>`}</div></div>`).join('')}${sharedLibraryNote(libs.map((l) => l.path))}`,
        `<button type="button" class="btn sm" data-act="lib-add">${esc(t('Add a folder'))}</button>`)
        + group(t('Your games'), `<p class="set-note">${esc(t('Point the launcher at your ROMs, disc images and game backups. It shows which have recomps, sets them up with your copy, and lets you know when a new recomp comes out for one. Files are only read on this PC.'))}</p>
        ${(st.rom_dirs || []).length ? (st.rom_dirs || []).map((d) => `<div class="set-row"><div class="k"><b class="path">${esc(d)}</b><span>${esc(folderCounts(d))}</span></div><div class="acts"><button type="button" class="btn sm" data-act="folder" data-path="${esc(d)}">${esc(t('Open'))}</button><button type="button" class="btn sm danger" data-act="rom-remove" data-path="${esc(d)}">${esc(t('Remove'))}</button></div></div>`).join('') : `<p class="set-empty">${esc(t('No folders yet.'))}</p>`}
        ${nOwned ? `<p class="set-foot">${esc(tn(nOwned, '1 of your games has a recomp.', '{n} of your games have recomps.'))} <a href="#" data-go="home">${esc(t('See them on Home'))}</a></p>` : ''}`,
        `<span class="acts"><button type="button" class="btn sm" data-act="rom-scan">${esc(t('Look again'))}</button><button type="button" class="btn sm" data-act="rom-emus">${esc(t('From My Emulators'))}</button><button type="button" class="btn sm main" data-act="rom-add">${esc(t('Add a folder'))}</button></span>`)
        + group(t('Desktop shortcuts'), row(t('Shortcuts Go Through the Launcher'), t('Off: a game\'s desktop shortcut starts the game itself, for front ends like RetroBat. On: it opens the launcher, which brings your shader, cloud saves and play time along.'), sw('shortcut_launcher', ui.shortcut_launcher, t('Shortcuts Go Through the Launcher'), 'ui-toggle')))
        + (S.steam ? steamSyncGroup(group, row, sw) : '')
        + (S.os === 'windows' ? group(t('Picking files'), row(t('Use the Launcher\'s Own File Browser'), t('Off: Windows\' own window for picking files and folders. On: the launcher\'s, which marks the games it knows and can unzip.'), sw('own_picker', ui.own_picker, t('Use the Launcher\'s Own File Browser'), 'ui-toggle'))) : '')
        + group(t('Recomps you already have'), row(t('Bring them into the Library'), t('Games from another launcher, Steam or a folder. They update from here after that, in the same folder.'), `<button type="button" class="btn sm" data-act="mig-open">${esc(t('Bring Your Games'))}</button>`))
        + dlssGroup(group, row)
        + group(t('Game Art'), `<div class="set-row"><div class="k"><b>${esc(t('SteamGridDB API Key'))}</b><span>${esc(t('Optional. With your own free key from steamgriddb.com (Preferences → API), games you add from GitHub get covers, backgrounds and logos.'))}</span></div>
          <span class="acts"><input class="field sm" id="sgdb-key" type="password" autocomplete="off" spellcheck="false" value="${esc(ui.sgdb_key || '')}" aria-label="${esc(t('SteamGridDB API Key'))}"><button type="button" class="btn sm" data-act="sgdb-save">${esc(t('Save'))}</button></span></div>`);
      break;
    case 'bring':
      body = window.migratePage ? window.migratePage(group) : '';
      break;
    case 'updates':
      body = group(t('Updates'), row(t('Check for Updates'), t('Asks each installed game\'s GitHub for a newer release now.'), `<button type="button" class="btn sm main" data-act="check-upd">${esc(t('Check now'))}</button>`)
          + row(t('Look for updates when the launcher opens'), t('Asks each installed game\'s GitHub for a newer release. Turn on Update automatically for a game in its Manage tab.'), sw('check_updates', st.check_updates, t('Look for updates')))
          + row(t('Back up saves before updates'), t('Keeps the newest five copies of each game\'s saves and settings.'), sw('backup_before_update', st.backup_before_update, t('Back up before updates'))))
        + group(t('Downloads'), row(t('Keep downloaded files'), t('Off: the zip is deleted once it\'s unpacked. On: it stays in the library\'s .downloads folder.'), sw('keep_downloads', st.keep_downloads, t('Keep downloaded files')))
          + row(t('Speed Limit'), t('Keeps downloads under this speed, so the rest of your home\'s internet stays quick.'), `<select class="sel" data-act="dl-limit" aria-label="${esc(t('Speed Limit'))}">${DL_LIMITS.map((n) => `<option value="${n}"${Number(ui.dl_limit || 0) === n ? ' selected' : ''}>${esc(n ? tf('{n} MB/s', { n }) : t('No Limit'))}</option>`).join('')}</select>`)
          + row(t('Slow Down While Playing'), t('While a game is running, downloads keep to 4 MB/s so the game gets your internet and drive first.'), sw('dl_slow_play', ui.dl_slow_play !== false, t('Slow Down While Playing'), 'dl-slow')))
        + networkGroup(group, row, st);
      break;
    case 'playing': {
      const hh = !!S.handheld, eff = (k) => (st[k] == null ? hh : !!st[k]);
      body = group(t('While Playing'), (hh ? `<p class="set-note">${esc(t('This PC is a handheld, so the launcher keeps out of the way while you play. Change it here.'))}</p>` : '')
          + (S.os === 'macos' ? `<p class="set-note">${esc(t('The in-game quick menu and screenshot capture are not available on Mac yet.'))}</p>` : row(t('Quick Menu Over Games'), t('Hold View on your controller (or press Ctrl+F12) for screenshots, settings and closing the game. Off: the launcher leaves your controller and keyboard alone while you play.'), sw('quick_menu', st.quick_menu !== false, t('Quick Menu Over Games'))))
          + row(t('Minimize the Launcher While Playing'), t('The launcher goes away while a game runs and comes back when it closes.'), sw('minimize_playing', eff('minimize_playing'), t('Minimize the Launcher While Playing')))
          + row(t('Lighter Launcher'), t('Still pictures and no animations, and nothing moves on Home while a game runs. Good for handhelds and older PCs.'), sw('lighter', eff('lighter'), t('Lighter Launcher'))));
      break;
    }
    case 'look':
      body = group(t('Appearance'), `<div class="set-row"><div class="k"><b>${esc(t('Theme'))}</b><span>${esc(t('Dark, black for OLED screens, or light.'))}</span></div><div class="seg" role="radiogroup" aria-label="${esc(t('Theme'))}">${[['dark', t('Dark')], ['oled', t('OLED')], ['light', tc('Light|theme')]].map(([k, n]) => `<button type="button" role="radio" aria-checked="${(ui.theme === 'light' ? 'light' : ui.oled ? 'oled' : 'dark') === k}" data-act="theme" data-k="${k}">${esc(n)}</button>`).join('')}</div></div>`
        + `<div class="set-row"><div class="k"><b>${esc(t('Text Size'))}</b><span>${esc(t('Bigger or smaller text, buttons and covers. Ctrl with + or - works too.'))}</span></div><div class="seg" role="radiogroup" aria-label="${esc(t('Text Size'))}">${UI_SCALES.map((k) => `<button type="button" role="radio" aria-checked="${uiScale() === k}" data-act="ui-scale" data-k="${k}">${Math.round(k * 100)}%</button>`).join('')}</div></div>`
        + `<div class="set-row"><div class="k"><b>${esc(t('Opens On'))}</b><span>${esc(t('The page you see when the launcher opens.'))}</span></div><div class="seg" role="radiogroup" aria-label="${esc(t('Opens On'))}">${[['', t('Home')], ['discover', t('Discover')], ['library', t('Library')]].map(([k, n]) => `<button type="button" role="radio" aria-checked="${(ui.start_page || '') === k}" data-act="start-page" data-k="${k}">${esc(n)}</button>`).join('')}</div></div>`
        + row(t('Compact sidebar'), t('Only the icons and your pinned games\' covers. The button at the bottom of the sidebar does the same.'), sw('mini', ui.mini, t('Compact sidebar'), 'ui-toggle'))
        + row(t('Cover Size Slider'), t('The slider at the bottom right of the Library, for the size of its covers and to switch to wide tiles.'), sw('hide_tile_ctl', !ui.hide_tile_ctl, t('Cover Size Slider'), 'ui-toggle'))
        + row(t('Covers Only'), t('Hide the names under game covers in the Library and on Home.'), sw('hide_names', ui.hide_names, t('Covers Only'), 'ui-toggle'))
        + row(t('Show Game Names'), t('Name every recomp after its game, like Tears of the Kingdom instead of TotK Recomp. Names you gave games yourself stay.'), sw('game_names', ui.game_names, t('Show Game Names'), 'ui-toggle'))
        + row(t('Show Game Media'), t('Every version of a game shows the same cover, background, logo and icon: the recommended version\'s. The versions list keeps each one\'s own icon, and pictures you picked yourself stay.'), sw('game_media', ui.game_media, t('Show Game Media'), 'ui-toggle'))
        // Linux: how the window is drawn, read when the launcher starts.
        + (S.os === 'linux' ? `<div class="set-row"><div class="k"><b>${esc(t('Graphics Mode'))}</b><span>${esc(t('How the launcher draws its window. Faster is smoother; Safer fixes a blank or flickering window. Takes effect the next time the launcher opens.'))}</span></div><div class="seg" role="radiogroup" aria-label="${esc(t('Graphics Mode'))}">${[['', t('Auto')], ['fast', t('Faster')], ['safe', t('Safer')]].map(([k, n]) => `<button type="button" role="radio" aria-checked="${(st.linux_graphics || '') === k}" data-act="gfx-mode" data-k="${k}">${esc(n)}</button>`).join('')}</div></div>` : ''))
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
      body = group(t('Controllers'), (S.pads || []).length ? S.pads.map((p) => `<div class="set-row"><div class="k"><b>${esc(p.name)}</b><span>${esc(p.kind === 'ps' ? t('PlayStation controller') : p.kind === 'nintendo' ? t('Nintendo controller') : p.kind === 'steam' ? t('Steam controller. With Steam open, Steam Input passes its buttons to the launcher and your games.') : t('Xbox controller'))}</span></div>${ok(true)}</div>`).join('') : `<p class="set-empty">${esc(t('None connected. Xbox, PlayStation (DualSense and DualShock 4) and Switch Pro controllers work, plugged in or over Bluetooth, and so do 8BitDo pads. A Steam Controller works with Steam open.'))}</p>`)
        + group(t('Moving Around'), row(t('Move with the left stick'), t('The left stick moves around the launcher like the d-pad. Turn it off if your stick drifts or you use it as a mouse; the d-pad still works.'), sw('pad_stick', st.pad_stick !== false, t('Move with the left stick'))))
        + group(t('Buttons'), `<div class="pad-guide">${buttonGuide().map(([k, n, d]) => `<div><span class="pg-k">${k}</span><span class="pg-t"><b>${esc(n)}</b><small>${esc(d)}</small></span></div>`).join('')}</div>`);
      break;
    case 'pc':
      body = pc ? group(t('Your PC'), `${pc.vcredist != null ? `<div class="set-row"><div class="k"><b>${esc(t('Microsoft Visual C++ runtime'))}</b><span>${esc(pc.vcredist && pc.vcredist_old ? tf('Out of date ({version}). Newer recomps need a newer one.', { version: pc.vcredist_old }) : t('Almost every Windows recomp needs it.'))}</span></div><div class="acts">${pc.vcredist && !pc.vcredist_old ? '' : `<button type="button" class="btn sm main" data-act="vcredist">${esc(pc.vcredist ? t('Update It') : t('Get it'))}</button>`}${ok(pc.vcredist && !pc.vcredist_old)}</div></div>` : ''}
        ${pc.vulkan != null ? `<div class="set-row"><div class="k"><b>Vulkan</b><span>${esc(t('Comes with your graphics driver. N64 recomps draw with it.'))}</span></div>${ok(pc.vulkan)}</div>` : ''}
        ${pc.vkbasalt != null ? `<div class="set-row"><div class="k"><b>vkBasalt</b><span>${esc(t('Draws shaders over games on Linux. It comes from your distro\'s packages, usually as vkbasalt.'))}</span></div>${ok(pc.vkbasalt)}</div>` : ''}
        ${pc.proton != null ? `<div class="set-row"><div class="k"><b>Proton${pc.proton ? ' · ' + esc(pc.proton) : ''}</b><span>${esc(t('Plays recomps that only have a Windows version. Steam\'s Proton, GE-Proton, umu-launcher or Wine.'))}</span></div>${ok(!!pc.proton)}</div>` : ''}
        ${pc.proton ? `<div class="set-row"><div class="k"><b>${esc(t('Wine Tools'))}</b><span>${esc(t('Winetricks and installers for the Wine prefix every Windows game here shares, for when a game says something is missing.'))}</span></div><button type="button" class="btn sm" data-act="wine-tools">${esc(t('Open'))}</button></div>` : ''}
        ${S.os === 'macos' ? '' : `<div class="set-row"><div class="k"><b>AVX2</b><span>${esc(t('Some Xbox 360 recomps need a processor from 2013 or later.'))}</span></div>${ok(pc.avx2)}</div>`}`) : group(t('Your PC'), `<p class="set-empty">${esc(t('Loading…'))}</p>`);
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
  // The controller's highlight stays on through a redraw.
  const lit = o.classList.contains('pad-focus');
  for (const a of Array.from(n.attributes)) if (o.getAttribute(a.name) !== a.value) o.setAttribute(a.name, a.value);
  if (lit && !o.classList.contains('pad-focus')) o.classList.add('pad-focus');
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
// The tile (or download row) the page keeps in place across a redraw: counted among tiles and rows
// only, since a download adds and takes away buttons that carry the game's id too, and a game shown
// once on the page first (one in two rows can't be told apart after a row changes).
const ANCHORS = '.tile[data-id], .dl-row[data-id]';
const anchorsOf = (main, id) => $$(ANCHORS, main).filter((e) => e.dataset.id === id);
function scrollAnchor(main) {
  const mr = main.getBoundingClientRect();
  let first = null;
  for (const el of $$(ANCHORS, main)) {
    const r = el.getBoundingClientRect();
    if (r.bottom > mr.top + 60 && r.top < mr.bottom && r.width) {
      const id = el.dataset.id, all = anchorsOf(main, id);
      const a = { id, n: all.indexOf(el), y: r.top, one: all.length === 1 };
      if (a.one) return a;
      first = first || a;
    }
  }
  return first;
}
function render(keepScroll) {
  // Records set from an answer about one of a recomp's games go back on the recomp, and its games' records follow.
  expandLib();
  const main = $('#main'), top = main.scrollTop;
  const v = S.view;
  // Typing in a search: it keeps its text, caret and focus across a redraw.
  const ae = document.activeElement, typing = ae && (ae.id === 'dq' || ae.id === 'lq') ? { id: ae.id, v: ae.value, a: ae.selectionStart, b: ae.selectionEnd } : null;
  if (ae && ae.id === 'dqs') S.q = ae.value;
  if (typing) { if (typing.id === 'dq') S.q = typing.v; else S.lq = typing.v; }
  const gp = $('.gp', main);
  // A plugin's page keeps running while the rest of the window is redrawn.
  if (v.startsWith('plug:') && !S.anim && main.dataset.v === v) { renderSide(); paintIsland(); $('#offline').hidden = !offNow(); return; }
  if (keepScroll && v === 'game' && !S.anim && gp && gp.dataset.gp === S.arg) { refreshGame(); renderSide(); paintIsland(); $('#offline').hidden = !offNow(); fitTitles(main); return; }
  const html = v === 'discover' ? pageDiscover() : v === 'library' ? pageLibrary() : v === 'achievements' ? pageAchievements() : v === 'game' ? pageGame(S.arg) : v === 'downloads' ? pageDownloads() : v === 'settings' ? pageSettings() : v === 'shots' ? pageShots() : v === 'soon' ? pageSoon() : v === 'saves' && window.pageSaves ? window.pageSaves() : v.startsWith('plug:') && window.pagePlugin ? window.pagePlugin(v.slice(5)) : pageHome();
  // Going to a page draws it fresh, with its entrance. Anything else (the same page with new
  // data) is patched in place, so nothing on screen flickers.
  // Redrawing the same page: whatever was at the top of the view stays there, even when something
  // above it came or went (a row, a badge, a banner).
  const anchor = keepScroll && !S.anim && main.dataset.v === v ? scrollAnchor(main) : null;
  if (S.anim || main.dataset.v !== v) main.innerHTML = html;
  else morph(main, html);
  main.dataset.v = v;
  if (S.anim) {
    main.classList.remove('enter', 'enter-game', 'enter-soft', 'enter-back');
    // Flush styles (not layout) so the entrance animation restarts.
    getComputedStyle(main).animationName;
    main.classList.add(S.anim);
    let i = 0;
    for (const el of $$('.grid > .tile, .shotgrid > .shot', main)) el.style.setProperty('--i', Math.min(i++, 18));
    S.anim = null;
  }
  renderSide();
  paintIsland();
  paintTileCtl();
  paintStick();
  $('#offline').hidden = !offNow();
  evenGrids(main);
  layoutShelves(main);
  fitTitles(main);
  // Back to where it was once the page has its final shape (the grids' columns are set just above:
  // measured before that, a tile is somewhere it won't stay).
  if (keepScroll) main.scrollTop = top;
  if (anchor) { const all = anchorsOf(main, anchor.id); const el = anchor.one ? (all.length === 1 ? all[0] : null) : all[anchor.n]; if (el) main.scrollTop += el.getBoundingClientRect().top - anchor.y; }
  moveTabPill();
  stickCheck();
  watchMore();
  if (v === 'game') paintLooks();
  if (typing) { const f = $('#' + typing.id); if (f) { f.focus({ preventScroll: true }); try { f.setSelectionRange(typing.a, typing.b); } catch (e) { /* fine */ } } }
}
// The Library's tile size and view: pinned to the window's bottom-right corner, outside the page,
// so a page's entrance, a redraw or a resized window never moves it. Only on the Library.
function paintTileCtl() {
  let c = document.getElementById('tilectl');
  const want = S.view === 'library' && !!$('#llist') && !(S.settings.ui || {}).hide_tile_ctl;
  if (!want) { if (c) c.remove(); return; }
  const key = [curLang(), libWide()].join('|');
  if (!c) { c = document.createElement('div'); c.id = 'tilectl'; c.className = 'tile-size'; $('#app').append(c); }
  if (c.dataset.k !== key) { c.dataset.k = key; c.innerHTML = `<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><rect x="5" y="5" width="6" height="6" rx="1"/><rect x="13" y="13" width="6" height="6" rx="1"/></svg><input type="range" min="120" max="360" step="10" value="${libTile()}" data-act="tile-size" aria-label="${esc(t('Tile size'))}" title="${esc(t('Tile size'))}"><button type="button" class="ts-view" data-act="libview" aria-label="${esc(libWide() ? t('Show covers') : t('Show wide tiles'))}" title="${esc(libWide() ? t('Show covers') : t('Show wide tiles'))}">${libWide() ? VIEW_WIDE : VIEW_COVERS}</button>`; }
  // Its value follows the size, except while it's being dragged.
  const r = $('input', c);
  if (r && document.activeElement !== r && Number(r.value) !== libTile()) r.value = libTile();
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
  // A new launcher: its own button at the foot of the sidebar too.
  { const su = $('#side-upd'); if (su) su.hidden = !S.launcherUpd; }
  if (window.achSide) window.achSide();
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
  const html = list.map((a) => `<button type="button" class="pin${v === 'game' && S.arg === a.id ? ' on' : ''}" data-act="open" data-id="${esc(a.id)}" title="${esc(a.name)}"><span class="th">${a.cover ? pic(thumbOf(a.cover, 160)) : ''}${S.running.has(a.id) ? '<i class="run"></i>' : ''}</span><span class="lbl">${esc(a.name)}</span></button>`).join('');
  const box = $('#pins');
  if (box.dataset.html !== html) { box.innerHTML = html; box.dataset.html = html; }
  for (const b of $$('.pin', box)) if (!iconsOnly) b.removeAttribute('title');
}
// The pinned games in a new order. Pins the launcher can't show right now (a catalog that hasn't
// loaded yet) keep their place at the end instead of being dropped.
function savePinOrder(shown) {
  const all = (S.settings.ui || {}).pins || [];
  const rest = all.filter((id) => !shown.includes(id));
  saveUi({ pins: [...shown, ...rest] }, true);
  renderSide();
}
// Moving a pinned game: dragged with the mouse (a press that moves a few pixels; a plain click still
// opens the game), or Alt+Up/Down on the focused one. Pointer events rather than HTML drag and drop,
// which the web view hands to the system's file drop on some PCs.
function pinDrag() {
  const box = $('#pins');
  let drag = null, eatClick = false;
  const rows = () => $$('.pin', box);
  box.addEventListener('pointerdown', (e) => {
    const pin = e.target.closest('.pin');
    if (!pin || e.button !== 0 || rows().length < 2) return;
    drag = { pin, id: pin.dataset.id, y: e.clientY, on: false, pointer: e.pointerId, to: 0 };
  });
  box.addEventListener('pointermove', (e) => {
    if (!drag || e.pointerId !== drag.pointer) return;
    const dy = e.clientY - drag.y;
    if (!drag.on && Math.abs(dy) < 6) return;
    if (!drag.on) {
      drag.on = true;
      drag.list = rows();
      drag.from = drag.list.indexOf(drag.pin);
      drag.step = drag.pin.offsetHeight + (parseFloat(getComputedStyle(box).rowGap) || 0);
      drag.pin.setPointerCapture(e.pointerId);
      box.classList.add('sorting');
      drag.pin.classList.add('moving');
    }
    // The dragged game follows the pointer; the others slide aside to show where it lands.
    const lowest = -drag.from * drag.step, highest = (drag.list.length - 1 - drag.from) * drag.step;
    const shift = Math.max(lowest, Math.min(highest, dy));
    drag.to = drag.from + Math.round(shift / drag.step);
    drag.pin.style.transform = `translateY(${shift}px)`;
    drag.list.forEach((b, i) => {
      if (b === drag.pin) return;
      const down = drag.to < drag.from && i >= drag.to && i < drag.from;
      const up = drag.to > drag.from && i <= drag.to && i > drag.from;
      b.style.transform = down ? `translateY(${drag.step}px)` : up ? `translateY(${-drag.step}px)` : '';
    });
  });
  const drop = (e) => {
    if (!drag || e.pointerId !== drag.pointer) return;
    const d = drag;
    drag = null;
    if (!d.on) return;
    // The click that ends a drag isn't a click on the game.
    eatClick = true;
    setTimeout(() => { eatClick = false; }, 0);
    box.classList.remove('sorting');
    d.pin.classList.remove('moving');
    d.list.forEach((b) => { b.style.transform = ''; });
    if (e.type === 'pointercancel' || d.to === d.from) return;
    const ids = d.list.map((b) => b.dataset.id);
    ids.splice(d.to, 0, ids.splice(d.from, 1)[0]);
    savePinOrder(ids);
  };
  box.addEventListener('pointerup', drop);
  box.addEventListener('pointercancel', drop);
  box.addEventListener('click', (e) => {
    if (!eatClick) return;
    e.preventDefault();
    e.stopPropagation();
  }, true);
  box.addEventListener('keydown', (e) => {
    const pin = e.target.closest('.pin');
    if (!pin || !e.altKey || (e.key !== 'ArrowUp' && e.key !== 'ArrowDown')) return;
    e.preventDefault();
    const ids = rows().map((b) => b.dataset.id);
    const from = ids.indexOf(pin.dataset.id), to = from + (e.key === 'ArrowUp' ? -1 : 1);
    if (to < 0 || to >= ids.length) return;
    ids.splice(to, 0, ids.splice(from, 1)[0]);
    savePinOrder(ids);
    const moved = $(`.pin[data-id="${CSS.escape(pin.dataset.id)}"]`, box);
    if (moved) moved.focus();
  });
}
pinDrag();
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
    el.innerHTML = `<span class="th">${a.cover ? pic(thumbOf(a.cover, 160)) : ''}</span><span class="k"><b></b><small></small></span><span class="pc"></span><i class="ln"></i>`;
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
    const n = Number(g.dataset.n) || g.children.length, min = g.closest('#llist') ? (Number(getComputedStyle(g.closest('#llist')).getPropertyValue('--tile')) || libTile()) * (g.closest('#llist.wide') ? 1.7 : 1) : g.classList.contains('shotgrid') ? 260 : g.classList.contains('compact') ? 140 : 160;
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
// With `opts.exit`, the tiles a filter takes out fade away where they stood while the rest glides over.
function morphList(box, html, opts = {}) {
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
    // The grid that stays never leaves this page: a picture moved into the template and back
    // can be left blank (the covers and logos that went missing in the Library).
    const keep = oldGrid, stay = new Set(kids);
    // The tiles on their way out, measured while they still stand in place (the ones on screen only).
    const gridBox = keep.getBoundingClientRect();
    const leaving = opts.exit && !calm() ? [...old.values()].filter((el) => !stay.has(el) && before.has(el)).slice(0, 24).map((el) => ({ el, r: el.getBoundingClientRect() })) : [];
    for (const { el } of leaving) before.delete(el);
    // Only what has to move moves: a tile that is already in its place is never taken out of the page
    // (taking it out and back in makes its cover blink), so only the ones that change place glide.
    for (const c of [...keep.childNodes]) if (!stay.has(c)) c.remove();
    let slot = keep.firstChild;
    for (const k of kids) { if (k === slot) slot = slot.nextSibling; else keep.insertBefore(k, slot); }
    if (leaving.length) {
      keep.style.position = 'relative';
      const done = leaving.map(({ el, r }) => {
        Object.assign(el.style, { position: 'absolute', left: `${r.left - gridBox.left}px`, top: `${r.top - gridBox.top}px`, width: `${r.width}px`, height: `${r.height}px`, margin: '0', pointerEvents: 'none' });
        keep.append(el);
        return el.animate([{ opacity: 1, transform: 'none' }, { opacity: 0, transform: 'scale(.94)' }], { duration: 200, easing: EASE, fill: 'forwards' }).finished.catch(() => {}).then(() => el.remove());
      });
      Promise.all(done).then(() => { keep.style.position = ''; });
    }
    if (keep.parentNode === box && newGrid.parentNode === tpl.content) {
      const nodes = [...tpl.content.childNodes], at = nodes.indexOf(newGrid);
      for (const c of [...box.childNodes]) if (c !== keep) c.remove();
      keep.before(...nodes.slice(0, at));
      keep.after(...nodes.slice(at + 1));
    } else {
      const mark = document.createComment('');
      newGrid.replaceWith(mark);
      box.replaceChildren(...tpl.content.childNodes);
      mark.replaceWith(keep);
    }
    evenGrids(box);
    fitTitles(box);
    glideFrom(before);
    reviveSoon(box);
    watchMore();
  } else {
    box.replaceChildren(...tpl.content.childNodes);
    fresh.push(...$$('.grid > .tile', box));
    evenGrids(box);
    fitTitles(box);
    reviveSoon(box);
    watchMore();
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
  const dk = $('.dctl [data-act="deck-f"]');
  if (dk) dk.setAttribute('aria-checked', String(!!f.deckgreat));
  const en = $('.dctl [data-act="ended-f"]');
  if (en) en.setAttribute('aria-checked', String(!!f.hideEnded));
  const ai = $('.dctl [data-k="hide_ai"]');
  if (ai) ai.setAttribute('aria-checked', String(!!S.settings.hide_ai));
  const h = $('.dhead h1');
  if (h) h.textContent = S.q.trim() ? t('Search') : f.genre ? t(GENRES[f.genre]) : t('Discover');
  const vals = { origin: f.origin || '', 'genre-sel': f.genre || '', 'show-sel': f.show || '' };
  for (const sl of $$('#main .dctl select[data-act]')) if (sl.dataset.act in vals && sl.value !== vals[sl.dataset.act]) sl.value = vals[sl.dataset.act];
  if (window.syncDrops) window.syncDrops();
  const hadEnded = !!$('#dlist .ended-box');
  morphList($('#dlist'), discoverGrid(), { exit: true });
  // The "Ended projects" box that has just appeared eases in after the list has settled, instead of popping.
  const box = $('#dlist .ended-box');
  if (box && !hadEnded && !calm()) box.animate([{ opacity: 0, transform: 'translateY(10px)' }, { opacity: 1, transform: 'none' }], { duration: 360, easing: EASE, delay: 160, fill: 'backwards' });
  paintStick();
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
    // Only the games in view glide; the rest just take their new places.
    const box = main.getBoundingClientRect();
    const inView = $$('.grid > .tile, .shelf .track > *', main).filter((el) => { const r = el.getBoundingClientRect(); return r.bottom > box.top - 200 && r.top < box.bottom + 200; });
    const before = spots(inView, main);
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
    // Discover opens at the top, so it starts again from the first page of its list.
    if (view === 'discover') S.dshow = DISCOVER_PAGE;
    // Reset before drawing: after it, the browser would have to lay the new page out again
    // just to clamp the old scroll position.
    $('#main').scrollTop = 0;
    S.anim = view === 'game' ? 'enter-game' : 'enter';
    render();
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
  Object.assign(S, { os: st.os, arch: st.arch, flatpak: !!st.flatpak, version: st.version, site: st.site, thumbs: st.thumbs || '', library: st.library || {}, arts: st.arts || {}, settings: st.settings || {}, roms: st.roms || [], steam: !!st.steam, deck: !!st.deck, bigArg: !!st.bigArg, desktopArg: !!st.desktopArg, noSound: !!st.noSound });
  document.body.classList.toggle('linux', S.os === 'linux');
  document.body.classList.toggle('mac', S.os === 'macos');
  document.body.classList.toggle('cpu-draw', !!st.cpuDraw);
  S.running = new Set(st.running || []);
  document.body.classList.toggle('playing', S.running.size > 0);
  if (st.updateFailed && !S.updateFailedShown) { S.updateFailedShown = true; setTimeout(() => updateFailedModal(st.updateFailed), 1500); }
  S.missing = new Set(st.missing || []);
  // The games of recomps that carry several: asked again when what's installed changes.
  const gk = Object.values(S.library).map((g) => g.id + '@' + g.tag).sort().join('|');
  if (S.gameKey !== gk) { S.gameKey = gk; await loadGameSets(); S.gameSetsV = (S.gameSetsV || 0) + 1; } else expandLib();
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
    // The art of the games in the Library is kept on disk, so it shows offline too. The rest comes
    // as it's shown (and is kept from then on).
    const urls = uniq(S.cat.apps.filter((a) => S.library[a.id]).flatMap((a) => [thumbOf(a.cover, 160), thumbOf(a.cover), a.hero, a.logo]).filter(Boolean).map(webUrl)).filter((u) => /^https?:/.test(u));
    if (urls.length) invoke('prefetch_images', { urls }).catch(() => {});
    // The list says there's no version for this system: check the releases themselves.
    // Only for entries that don't say which systems they're for: the list's own answer is trusted.
    const ask = S.cat.apps.filter((a) => a.repo && !a.soon && !(a.systems || []).length && !(a.assets && Object.keys(a.assets).length)).map((a) => a.id);
    if (ask.length) invoke('build_check', { ids: ask }).then((b) => { S.builds = Object.assign({}, S.builds, b); if (Object.values(b).some(Boolean)) { render(true); if (window.tvRefresh) window.tvRefresh(); } }).catch(() => {});
  } catch (e) {
    // No list on disk and the site can't be reached: the Library still works, and the "Offline"
    // mark says why Discover is empty. No popup.
    if (!S.cat) S.cat = { apps: [], origins: [] };
    S.offline = true;
  }
}
// A newer list came in the background (it was more than six hours old, or the site said it changed).
T.event.listen('catalog-fresh', () => loadCatalog(false).then(() => render(true)).catch(() => {}));
// A game the player added by its link is on the list now: their copy moved over to the list's one.
T.event.listen('mine-merged', async (e) => {
  const list = (e && e.payload) || [];
  try { await refreshState(); } catch (err) { /* it shows on the next look */ }
  for (const m of list) {
    toast(tf('{name} is on Discover now. Your copy moved over to it.', { name: m.name || m.to }));
    if (S.arg === m.from) S.arg = m.to;
    if (window.tvRemap) window.tvRemap(m.from, m.to);
  }
  render(true);
  if (window.tvRefresh && document.body.classList.contains('tv-on')) window.tvRefresh();
});
// A recomp's releases came in fresh in the background.
T.event.listen('releases-fresh', ({ payload }) => { const repo = payload && payload.repo; if (repo && S.rels[repo] && !S.rels[repo].loading) { delete S.rels[repo]; loadRels(repo); } });

async function loadRels(repo, force) {
  if (!repo || (S.rels[repo] && S.rels[repo].loading)) return;
  const previous = (S.rels[repo] || {}).archived;
  S.rels[repo] = { loading: true, list: [], archived: previous };
  let statusChanged = false;
  try {
    const r = await invoke('get_releases', { repo, force: !!force });
    const archived = typeof r.archived === 'boolean' ? r.archived : previous;
    S.rels[repo] = { list: Array.isArray(r.data) ? r.data : [], archived };
    statusChanged = archived !== previous;
  } catch (e) {
    S.rels[repo] = { list: [], error: String(e), archived: previous };
  }
  if (S.view === 'game') refreshGame();
  else if (S.view === 'downloads') for (const el of $$('.upd.open')) fillUpd(el);
  else if (statusChanged && ['home', 'discover', 'library'].includes(S.view)) render(true);
  if (window.tvRefresh && document.body.classList.contains('tv-on')) window.tvRefresh();
}

// Like the website: when the pointer rests on a game (or it gets focus), what its page shows from
// the site starts loading, so the page has it when it opens. Once per game per session.
let warmT = 0;
function warmGame(el) {
  const id = el.dataset.id, a = id && byId(id);
  if (!a || S.view === 'game') return;
  if (S.reports[id] === undefined && !a.soon && !(S.library[id] && S.library[id].local)) loadReports(id);
  if (a.repo && !S.rels[a.repo]) loadRels(a.repo);
}
for (const ev of ['pointerover', 'focusin']) {
  document.addEventListener(ev, (e) => {
    const el = e.target.closest && e.target.closest('[data-act="open"][data-id]');
    clearTimeout(warmT);
    if (el && !document.body.classList.contains('tv-on')) warmT = setTimeout(() => warmGame(el), 300);
  });
}

let updateCheckRevision = 0;
async function checkUpdates(loud) {
  const revision = updateCheckRevision;
  try {
    const updates = await invoke('check_updates', { force: !!loud });
    if (revision !== updateCheckRevision) return false;
    S.updates = Object.fromEntries(Object.entries(updates).filter(([id]) => S.library[id] && !S.library[id].local));
    S.updInfo = {};
  } catch (e) { if (loud) fail(e); return false; }
  renderSide();
  if (window.tvRefresh && document.body.classList.contains('tv-on')) window.tvRefresh();
  if (S.view === 'downloads') render(true);
  // Never on its own to a version the site found crashing for a lot of players.
  const auto = Object.keys(S.updates).filter((id) => S.library[id] && S.library[id].auto_update && !S.running.has(id) && !S.jobs[id] && !(S.paused || {})[id] && !badVersion(byId(id), S.updates[id]));
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
  const { program, ...options } = opts;
  if (T.dialog && T.dialog.open) return T.dialog.open(options);
  return invoke('plugin:dialog|open', { options });
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
// A bundle is selectable only when choosing a program, never as a ROM or ordinary file.
function fbProgramBundle(opts, e) {
  return S.os === 'macos' && !!opts.program && !opts.directory && e.dir && /\.app$/i.test(e.name);
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
// Linux: some PCs draw the window slowly (the web view's safer way of drawing, mostly). Frames
// are timed only while something moves (a scroll, a key, a click, the controller), and once
// they're slow more often than not, the player hears about the two settings that help, once.
function watchSlow() {
  if ((S.os !== 'linux' && S.os !== 'macos') || (S.settings.ui || {}).slow_told) return;
  let busyUntil = 0, last = 0, running = false, frames = 0, slow = 0, sampled = 0, done = false;
  const tick = (now) => {
    if (done) return;
    if (now > busyUntil) { running = false; last = 0; return; }
    if (last) { const d = now - last; if (d < 1000) { frames++; sampled += d; if (d > 45) slow++; } }
    last = now;
    // Measure active rendering time: waiting for 300 frames takes longest when rendering is slow.
    if (sampled >= 2000 && frames >= 30) {
      if (slow / frames > 0.4 && !document.body.classList.contains('lighter') && !document.querySelector('.scrim')) { done = true; return tellSlow(); }
      frames = 0; slow = 0; sampled = 0;
    }
    requestAnimationFrame(tick);
  };
  window.pokeSlow = () => {
    if (done || document.hidden || S.running.size || document.body.classList.contains('tv-on')) return;
    busyUntil = performance.now() + 500;
    if (!running) { running = true; last = 0; requestAnimationFrame(tick); }
  };
  for (const e of ['wheel', 'scroll', 'keydown', 'pointerdown']) addEventListener(e, window.pokeSlow, { passive: true, capture: true });
}
function tellSlow() {
  saveUi({ slow_told: true }, true).catch(() => {});
  modal(`<h2>${esc(t('The Launcher Is Drawing Slowly Here'))}</h2>
    <p>${esc(S.os === 'macos' ? t('Scrolling and animations are choppy on this Mac. Lighter Launcher turns off the animations and moving pictures, which makes it smoother.') : t('Scrolling and animations are choppy on this PC. Lighter Launcher turns off the animations and moving pictures, which makes it smoother. If it\'s still choppy, try Graphics Mode: Faster in Settings → Appearance.'))}</p>
    <div class="acts"><button type="button" class="btn" data-x="no">${esc(t('Not Now'))}</button><button type="button" class="btn main" data-x="lighter">${esc(t('Turn On Lighter Launcher'))}</button></div>`, async (x, close) => {
    if (x.dataset.x !== 'lighter') return;
    close();
    await saveSettings({ lighter: true }, true);
    applyLighter();
    toast(t('Lighter Launcher is on. It\'s in Settings → While Playing.'));
  }, t('The Launcher Is Drawing Slowly Here'));
}
async function pickFile(opts) {
  // As a Flatpak, the launcher's own picker sees only the folders it's allowed: the system's
  // picker (the portal) reaches every folder, in TV mode too.
  if (S.flatpak && !opts.launcherPicker) return systemPicker(opts);
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
  if (e.dir && !fbProgramBundle(FB.opts, e)) return fbGo(e.path);
  if (!fbTakes(FB.opts, e) && FB_ARCHIVE.test(e.name)) return fbExtract();
  fbPick();
}
// The file that's selected, or for a folder picker the selected folder (or the one that's open).
function fbChoice() {
  const e = fbRows()[FB.sel];
  if (FB.opts.directory) return e && e.dir ? e.path : FB.path;
  return e && (!e.dir || fbProgramBundle(FB.opts, e)) && fbTakes(FB.opts, e) ? e.path : '';
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
  hide: '<path d="M3 3l18 18M10.6 6.1A9.6 9.6 0 0 1 12 6c5.5 0 9 6 9 6a16.5 16.5 0 0 1-3 3.7M6.5 7.6C4.3 9.2 3 12 3 12s3.5 6 9 6c1.4 0 2.8-.4 4-1M9.9 9.9a3 3 0 0 0 4.2 4.2"/>',
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
  // With a mouse, a group opens beside its row as the pointer rests on it, and the pointer slides
  // over to pick; a short wait lets it cross other rows on the way. Keys and touch open the group
  // in place, with a Back row.
  let fly = null, flyFor = null, flyT = 0;
  const mouse = matchMedia('(hover: hover)').matches;
  const closeFly = () => { clearTimeout(flyT); if (fly) fly.remove(); if (flyFor) flyFor.classList.remove('open'); fly = null; flyFor = null; };
  const openFly = (btn) => {
    clearTimeout(flyT);
    if (flyFor === btn || !btn.isConnected) return;
    closeFly();
    const cur = stack.length ? stack[stack.length - 1].list : items;
    const it = cur[Number(btn.dataset.sub)];
    if (!it || !it.sub) return;
    fly = document.createElement('div');
    fly.className = 'menu fly';
    fly.setAttribute('role', 'menu');
    fly.innerHTML = menuRows(it.sub.filter((x) => x === '-' || !x.sub));
    document.body.append(fly);
    flyFor = btn;
    btn.classList.add('open');
    const r = btn.getBoundingClientRect(), mr = m.getBoundingClientRect();
    let left = mr.right + 4;
    if (left + fly.offsetWidth > innerWidth - 10) left = mr.left - fly.offsetWidth - 4;
    fly.style.left = Math.max(10, left) + 'px';
    fly.style.top = Math.max(10, Math.min(innerHeight - fly.offsetHeight - 10, r.top - 6)) + 'px';
    fly.addEventListener('pointerenter', () => clearTimeout(flyT));
    fly.addEventListener('pointerleave', (e) => { if (!m.contains(e.relatedTarget)) flyT = setTimeout(closeFly, 350); });
  };
  m.addEventListener('pointerover', (e) => {
    if (!mouse || (e.pointerType && e.pointerType !== 'mouse' && e.pointerType !== 'pen')) return;
    const b = e.target.closest('button');
    if (!b) return;
    clearTimeout(flyT);
    if (b.classList.contains('has-sub')) flyT = setTimeout(() => openFly(b), fly ? 140 : 70);
    else if (fly) flyT = setTimeout(closeFly, 280);
  });
  m.addEventListener('pointerleave', (e) => { if (fly && !fly.contains(e.relatedTarget)) { clearTimeout(flyT); flyT = setTimeout(closeFly, 400); } });
  m.addEventListener('click', (e) => {
    const sub = e.target.closest('[data-sub]');
    const back = e.target.closest('[data-back]');
    if (!sub && !back) return;
    e.stopPropagation();
    // A mouse click on a group: its menu beside it, the same as resting on it.
    if (sub && mouse && e.detail > 0) return openFly(sub);
    closeFly();
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
      : st === 'get' ? (isSaved(id) ? { act: 'install', id, label: x && turnkey(x) ? t('Easy Install') : t('Install'), icon: x && turnkey(x) ? 'disc' : 'down' } : { act: 'addlib', id, label: t('Add to Library'), icon: 'plus' }) : null;
  const pinned = pins().includes(id);
  return [
    ...(first ? [first] : []),
    ...(!g && st === 'get' && !isSaved(id) ? [{ act: 'install', id, label: t('Add and Install'), icon: 'down' }] : []),
    ...(g && S.updates[id] ? [{ act: 'install', id, label: t('Update'), icon: 'down' }] : []),
    ...(S.view !== 'game' || S.arg !== id ? [{ act: 'open', id, label: t('Game page'), icon: 'page' }] : []),
    { act: 'pin', id, label: pinned ? t('Unpin from the sidebar') : t('Pin to the sidebar'), icon: 'pin' },
    ...(g ? [{ act: 'fav', id, label: favs().includes(id) ? t('Remove from Favorites') : t('Add to Favorites'), icon: 'star' }] : []),
    ...(!g && isSaved(id) ? [{ act: 'unsave', id, label: t('Remove from Library'), icon: 'bin' }] : []),
    ...(x && !x.custom ? [isHidden(id) ? { act: 'unhide', id, label: t('Show in Discover'), icon: 'hide' } : { act: 'hide', id, label: t('Hide from Discover'), icon: 'hide' }] : []),
    // Its own pictures and words before it's installed too (they stay when it is).
    ...(!g && x ? [{ act: 'customize', id, label: t('Customize…'), icon: 'shot' }] : []),
    ...(g ? ['-',
      { label: t('Manage'), icon: 'gear', sub: [
        { act: 'project-association', id, label: t('Project association…'), icon: 'layers' },
        ...(!g.local ? [{ act: 'auto-up', id, label: t('Update automatically'), icon: 'refresh', check: !!g.auto_update }] : []),
        { act: 'versions', id, label: t('Changelog and other versions'), icon: 'history' },
        ...(!g.local ? [{ act: 'build', id, label: t('Change Build…'), icon: 'layers' }] : []),
        { act: 'launch-opts', id, label: t('Launch options…'), icon: 'exe' },
        { act: 'setexe', id, label: t('Choose Program…'), icon: 'exe' },
        { act: 'manual', id, label: g.manual ? t('Set Up By Hand (On)…') : t('Set Up By Hand…'), icon: 'gear' },
        ...(S.easy.has(id) ? [{ act: 'setup', id, label: g.game_copy ? t('Change your copy…') : t('Insert your copy…'), icon: 'disc' }] : []),
        ...(looksHere() ? [{ act: 'looks', id, label: t('Shaders'), icon: 'look' }] : []),
        ...(S.mods[id] ? [{ act: 'mods', id, label: t('Mods'), icon: 'puzzle' }] : []),
        ...(S.os === 'linux' && g.proton ? [{ act: 'wine-tools', id, label: t('Wine Tools…'), icon: 'exe' }] : []),
      ] },
      { act: 'saves-game', id, label: t('Manage saves'), icon: 'save' },
      { act: 'customize', id, label: t('Customize…'), icon: 'shot' },
      ...(x && x.custom ? [{ act: 'edit-info', id, label: t('Edit Game Info…'), icon: 'page' }] : []),
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
      ...(x && x.repo ? [isSF(x.repo) ? { label: t('On SourceForge'), icon: 'web', sub: sfMenu(x.repo) } : { label: isLab(x.repo) ? t('On GitLab') : t('On GitHub'), icon: 'web', sub: [{ act: 'url', url: repoReadme(x.repo), label: t('Read me'), icon: 'page' }, { act: 'url', url: repoHome(x.repo), label: isLab(x.repo) ? t('Its GitLab') : t('Its GitHub'), icon: 'web' }] }] : []),
      ...(window.pluginMenu ? window.pluginMenu(id) : []),
      '-',
      { act: 'troubleshoot', id, label: t('Troubleshoot…'), icon: 'info' },
      { act: 'report', id, label: t('Report a Problem…'), icon: 'page' },
    ] : x && x.repo ? (isSF(x.repo) ? ['-', ...sfMenu(x.repo)] : ['-', { act: 'url', url: repoReadme(x.repo), label: t('Read me'), icon: 'page' }, { act: 'url', url: repoHome(x.repo), label: isLab(x.repo) ? t('Its GitLab') : t('Its GitHub'), icon: 'web' }]) : []),
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
  id = parentId(id);
  const g = S.library[id];
  // A recomp with several games: they all go with it.
  const many = hasGames(id) ? S.gameSets[id].games : null;
  const s = document.createElement('div');
  s.className = 'scrim';
  s.innerHTML = `<div class="modal" role="dialog" aria-modal="true" aria-labelledby="u-h"><h2 id="u-h">${esc(tf('Uninstall {name}?', { name: g.name }))}</h2>
    <p>${esc(t('It comes out of your Library. You can install it again from Discover any time.'))}</p>
    ${many ? `<p>${esc(tf('Its {n} games are uninstalled with it: {games}.', { n: many.length, games: many.map((x) => x.name).join(', ') }))}</p>` : ''}
    ${g.exe && /^([a-z]:[\\/]|\\\\|\/)/i.test(g.exe) && !String(g.exe).toLowerCase().startsWith(String(g.path).toLowerCase()) ? `<p class="muted">${esc(t('Its own installer put the game somewhere else. With the box ticked, its own uninstaller opens too.'))}</p>` : ''}
    ${g.installer ? `<label><input type="checkbox" id="u-unins" checked> ${esc(t('Run its uninstaller too?'))}</label><p class="muted">${esc(tf('Without it, only the Library forgets it. Its folder stays where it is: {path}', { path: g.path }))}</p>` : g.installer_setup ? '' : ownFolder(g) ? `<label><input type="checkbox" id="u-del" checked> ${esc(tf('Also delete its folder ({path}), saves and settings inside it too', { path: g.path }))}</label>` : `<p class="muted">${esc(tf('Its folder stays where it is: {path}', { path: g.path }))}</p>`}
    <div class="acts"><button type="button" class="btn" data-x="no">${esc(t('Keep it'))}</button><button type="button" class="btn danger" data-x="yes">${esc(t('Uninstall'))}</button></div></div>`;
  document.body.append(s);
  $('[data-x="no"]', s).focus();
  s.addEventListener('click', async (e) => {
    const x = e.target.closest('[data-x]');
    if (e.target === s || (x && x.dataset.x === 'no')) return s.remove();
    if (!x) return;
    const del = !!($('#u-del', s) || {}).checked, unins = !!($('#u-unins', s) || {}).checked;
    s.remove();
    if (unins) toast(tf('{name}\'s uninstaller is opening. Finish it there.', { name: g.name }));
    let r = {};
    try { r = (await invoke('uninstall', { id, deleteFiles: del, runUninstaller: unins })) || {}; } catch (err) { await refreshState(); render(true); return fail(err); }
    if (window.usageUninstall) window.usageUninstall(id);
    await keepOut(id);
    delete S.updates[id]; await refreshState(); render(true);
    // Some files couldn't go (still in use by something): say where they are.
    if (r.left) toast(tf('{name} is uninstalled, but a few of its files are still in use and stayed in {path}. Close whatever is using them (or restart), then delete that folder.', { name: g.name, path: r.path }), true);
    else if (del && r.kept && r.copy_stuck) toast(tf('{name} is uninstalled, but its folder stayed: your copy of the game is inside it and couldn\'t be moved out. It\'s in {path}.', { name: g.name, path: r.path }), true);
    else if (r.installer) toast(r.uninstaller ? tf('{name} is uninstalled.', { name: g.name }) : tf('{name} is out of your Library. Its folder stays in {path}.', { name: g.name, path: r.path }));
    else if (r.uninstaller) toast(tf('{name} is uninstalled. Its own uninstaller is open to remove the game from where it installed it.', { name: g.name }));
    else toast(tf('{name} is uninstalled.', { name: g.name }));
    // The player's own files that were inside its folder: said apart, whatever else happened.
    if (r.kept_copy) toast(tf('Your copy of the game was inside its folder, so it\'s kept in {path}.', { path: r.kept_copy }));
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

// As a Flatpak, a folder outside ~/Games and the drives comes through the system's picker as a
// shared path (/run/user/…/doc/…), which is slower: a note under the install folders says so.
function sharedLibraryNote(paths) {
  if (!S.flatpak || !paths.some((p) => /^\/run\/user\/\d+\/doc\//.test(p || ''))) return '';
  return `<div class="callout lib-note"><span class="ic"><svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${MI.info}</svg></span><div class="k"><span>${esc(t('For the best performance, install recomps in your Games folder or on another drive.'))}</span></div></div>`;
}
async function addLibrary() {
  const p = await pickFile({ title: t('A folder for games, on any drive'), directory: true }).catch(fail);
  if (!p) return null;
  const path = typeof p === 'string' ? p : p.path;
  await saveSettings({ library_dirs: uniq([...(S.settings.library_dirs || []), path]).filter((d) => d !== S.settings.library_dir) });
  return path;
}

// The first install asks where games go; after that, only when there's more than one library.
// What a recomp's catalog entry says it needs from the PC ("requires": {avx2, vulkan, disk_gb}) that
// this PC doesn't have. Room on the drive is checked on the library it's going into.
function pcLacks(a, dir) {
  const r = (a && a.requires) || {}, pc = S.pc || {}, out = [];
  if (r.avx2 && S.os !== 'macos' && pc.avx2 === false) out.push(t('A processor with AVX2 (most from 2013 or later)'));
  if (r.vulkan && pc.vulkan === false) out.push(t('Vulkan graphics (a graphics driver update usually adds it)'));
  const gb = Number(r.disk_gb) || 0;
  const lib = (S.libs || []).find((l) => (dir ? l.path === dir : l.default));
  if (gb > 0 && lib && lib.free != null && lib.free < gb * 1073741824) out.push(tf('{size} free on the drive ({free} free there now)', { size: fmtB(gb * 1073741824), free: fmtB(lib.free) }));
  return out;
}
// "I Already Have It" on a game's page: the player's own copy of the recomp, from a folder they
// pick, checked and added like Bring Your Recomps does (where it is, or moved into the Recomp
// Library). Its version isn't known, so the next update offers itself.
async function haveIt(id) {
  const a = byId(id);
  if (!a || S.library[id]) return;
  const p = await pickFile({ title: tf('The folder {name} is in', { name: a.name }), directory: true }).catch(fail);
  if (!p) return;
  let r;
  try { r = await invoke('adopt_check', { id, path: typeof p === 'string' ? p : p.path }); } catch (e) { return fail(e); }
  if (r.taken) return toast(tf('{name} is already in your Library.', { name: r.taken }), true);
  if (!r.fits) {
    const v = await ask(t('This Doesn\'t Look Like It'), `<p>${esc(tf('There\'s no {name} program in that folder.', { name: a.name }))}</p>`,
      [{ label: t('Cancel'), icon: 'chevL', v: null }, { label: t('Use It Anyway'), icon: 'check', v: 'yes', main: true }]);
    if (v !== 'yes') return;
  }
  const where = await ask(tf('Add {name}', { name: a.name }), `<p class="path">${esc(r.path)}</p>`,
    [{ label: t('Move It Into the Recomp Library'), sub: S.settings.library_dir || '', icon: 'move', v: 'move' }, { label: t('Play It From Where It Is'), sub: t('It stays in its folder'), icon: 'play', v: 'here', main: true }]);
  if (!where) return;
  let added = [];
  try { added = await invoke('adopt_add', { items: [{ id, path: r.path, exe: r.exe || null, tag: r.tag || '', proton: !!r.proton }] }); } catch (e) { return fail(e); }
  if (!added.length) return toast(t('It couldn\'t be added from that folder.'), true);
  await refreshState();
  if (where === 'move') {
    toast(tf('Moving {name} into your Recomp Library…', { name: a.name }));
    try { await invoke('move_game', { id, to: S.settings.library_dir }); } catch (e) { fail(e); }
    await refreshState();
  }
  invoke('usage_event', { kind: 'setup', game: id, version: null, detail: 'added existing' }).catch(() => {});
  checkUpdates().then(() => render(true));
  toast(tf('{name} is in your Library. Press Play.', { name: a.name }));
  render(true);
  if (window.tvRefresh && document.body.classList.contains('tv-on')) window.tvRefresh();
}
window.haveIt = haveIt;
// Linux: Winetricks, an installer run in the shared Wine prefix, or its folder.
function wineTools() {
  const btn = (x, label, sub, main) => `<button type="button" class="btn bld${main ? ' main' : ''}" data-x="${x}"><b>${esc(label)}</b><small>${esc(sub)}</small></button>`;
  const btns = [btn('winetricks', t('Winetricks'), t('Add runtimes and libraries'), true), btn('run', t('Run a Program'), t('An installer, in the prefix')), btn('folder', t('Open Its Folder'), t('Where its drive_c folder is'))];
  btns[0] = btns[0].replace('class="btn', 'class="span2 btn');
  modal(`<h2>${esc(t('Wine Tools'))}</h2><p>${esc(t('Every Windows game here plays in one shared Wine prefix, so what you add is there for all of them. When a game says something is missing (Python, a runtime), add it here, then tell us what worked from Settings → Beta.'))}</p>
    <div class="acts grid2 builds">${btns.join('')}</div>`, async (x, close) => {
    const what = x.dataset.x;
    if (!what) return;
    let path = null;
    if (what === 'run') {
      const p = await pickFile({ title: t('A Windows program or installer'), filters: [{ name: t('Programs'), extensions: ['exe', 'msi'] }] }).catch(fail);
      if (!p) return;
      path = typeof p === 'string' ? p : p.path;
    }
    try {
      await invoke('wine_tools', { what, path });
      close();
      if (what === 'winetricks') toast(t('Winetricks is open. What you add goes in the shared prefix.'));
      else if (what === 'run') toast(t('It\'s running in the shared prefix.'));
    } catch (e) { fail(e); }
  }, t('Wine Tools'));
}
async function startInstall(id, dir, tag, anyway) {
  // A game of a recomp that carries several: the recomp installs (once), whichever game it's from.
  id = parentId(id);
  // An update of a recomp with several games: all of them get it.
  if (S.library[id] && hasGames(id) && S.updates[id] && !tag) toast(tf('Updating {name} updates all {n} of its games.', { name: S.library[id].name, n: S.gameSets[id].games.length }));
  if (dir === undefined && !S.library[id] && (!S.settings.asked_location || (S.settings.library_dirs || []).length)) {
    await loadLibs();
    return locationModal(id);
  }
  if (S.running.has(id)) return toast(t('Close the game first.'), true);
  // Easy Install with no copy found in the player's folders: pick it first, and it's checked against
  // what the game needs before anything downloads. Once it's in, setup goes straight on with it.
  if (!S.library[id] && !tag && !anyway && turnkey(byId(id)) && !S.owned[id] && !(S.prePicked || {})[id] && !document.body.classList.contains('tv-on')) {
    const pickIt = async () => {
      for (;;) {
        const f = await pickFile(copyPickOpts(id)).catch(() => null);
        if (!f) return null;
        const path = typeof f === 'string' ? f : f.path;
        try { await invoke('precheck_copy', { id, path }); return path; } catch (e) {
          const msg = String(e && e.message ? e.message : e);
          // Can't be checked before it's installed: nothing is kept, setup asks after the download.
          if (/only be checked once it's installed/i.test(msg)) return '';
          const v = await ask(t('That Copy Won\'t Work'), `<p>${esc(t(msg))}</p>`, [{ label: t('Pick Another'), icon: 'disc', v: 'again', main: true }, { label: t('Download Anyway'), icon: 'down', v: 'go' }, { label: t('Cancel'), icon: 'chevL', v: null }]);
          if (v === 'go') return '';
          if (v !== 'again') return null;
        }
      }
    };
    const first = await ask(t('Pick Your Copy First?'), `<p>${esc(t('The launcher checks it\'s the right game and version before it downloads anything. Then it sets the game up with it as soon as the download is done.'))}</p>`, [{ label: t('Pick My Copy'), icon: 'disc', v: 'pick', main: true }, { label: t('Download First'), icon: 'down', v: 'later' }]);
    if (first === 'pick') {
      const path = await pickIt();
      if (path === null) return;
      if (path) { (S.prePicked = S.prePicked || {})[id] = path; toast(t('That\'s the right copy. Downloading now.')); }
    } else if (first !== 'later') return;
  }
  // An update the site found crashing for a lot of players: asked once, the player decides.
  const badNext = S.library[id] && !tag && !anyway && S.updates[id] && badVersion(byId(id), S.updates[id]);
  if (badNext) {
    const v = await ask(t('This Update Is Crashing a Lot'), `<p>${esc(badNext.note || tf('{version} is crashing for a lot of players. The version you have keeps working.', { version: S.updates[id] }))}</p>`,
      [{ label: t('Keep My Version'), icon: 'chevL', v: null, main: true }, { label: t('Update Anyway'), icon: 'down', v: 'go' }]);
    if (v !== 'go') return;
  }
  // Before a first install: can this PC run it? The player can still go ahead.
  if (!S.library[id] && !anyway && byId(id) && byId(id).requires) {
    if (!S.pc) { try { S.pc = await invoke('pc_check'); } catch (e) { /* not known: nothing to warn about */ } }
    if (!S.libs) await loadLibs();
    const lacks = pcLacks(byId(id), dir);
    if (lacks.length) {
      const v = await ask(t('This PC May Not Run It'), `<p>${esc(t('It needs something this PC doesn\'t seem to have:'))}</p><ul class="lacks">${lacks.map((x) => `<li>${esc(x)}</li>`).join('')}</ul>`,
        [{ label: t('Cancel'), icon: 'chevL', v: null }, { label: t('Install Anyway'), sub: lacks.join(' · '), icon: 'down', v: 'go', main: true }]);
      if (v !== 'go') return;
    }
  }
  S.jobs[id] = { id, phase: 'prepare', done: 0, total: 0, at: Date.now() };
  render(true);
  dlArgs(id, { id, dir: dir || null, tag: tag || null });
  try { await invoke('install', { id, dir: dir || null, tag: tag || null }); } catch (err) { delete S.jobs[id]; fail(err); render(true); }
}
// What each download was started with, so Resume carries on with the same version and folder.
function dlArgs(id, args) { (S.dlArgs = S.dlArgs || {})[id] = args; }
// Carries on a paused download from where it stopped (the part file is still there).
function resumeDl(id) {
  const args = (S.dlArgs || {})[id] || { id, dir: null, tag: null };
  delete S.paused[id];
  S.jobs[id] = { id, phase: 'prepare', done: 0, total: 0, at: Date.now() };
  render(true);
  invoke('install', args).catch((e) => { delete S.jobs[id]; fail(e); render(true); });
}

// Play, closing another game first if one is running (after asking).
const STARTING = new Set();
// What a Setup Required recomp needs before it plays: its developer's steps, the README, and a way
// to say it's done (then Play is back).
// A release with more than one build for this PC (one per region, or an AppImage and a plain
// folder, say): the player picks which. Each is {name, label (the kind of build), best}.
const BUILD_KINDS = [N_('Flatpak'), N_('AppImage'), N_('Linux Folder'), N_('Installer'), N_('32-Bit Installer'), N_('Program'), N_('32-Bit Program'), N_('Portable'), N_('32-Bit Portable'), N_('Build'), N_('Windows Build')];
// "PICK_BUILD|*name<tab>kind|name<tab>kind" from an install: the usual pick has the *.
const buildList = (msg) => String(msg).split('|').slice(1).map((s) => { const best = s.startsWith('*'); const [name, label] = s.replace(/^\*/, '').split('\t'); return { name, label: label || '', best }; });
function pickBuild(id, builds, current) {
  const a = byId(id) || { name: id };
  const go = (file) => { S.jobs[id] = { id, phase: 'prepare', done: 0, total: 0, at: Date.now() }; render(true); dlArgs(id, { id, dir: null, tag: null, file }); invoke('install', { id, dir: null, tag: null, file }).catch((e) => { delete S.jobs[id]; fail(e); render(true); }); };
  if (window.tvPickBuild && window.tvPickBuild(id, builds, current, go)) return;
  // Kinds of build to choose from (a portable zip, an installer): each shows its kind, and the usual
  // one is marked. Builds of one kind (one per region): just their names, none suggested.
  const kinds = new Set(builds.map((b) => b.label)).size > 1;
  const lit = builds.some((b) => b.name === current) ? (b) => b.name === current : (b) => kinds && b.best;
  const btns = builds.map((b) => `<button type="button" class="btn bld${lit(b) ? ' main' : ''}" data-x="b" data-f="${esc(b.name)}" title="${esc(b.name)}">${kinds ? `<b>${esc(t(b.label))}${b.best ? ` <span class="bld-tag">${esc(t('Usual'))}</span>` : ''}</b><small dir="ltr">${esc(b.name)}</small>` : `<b dir="ltr">${esc(b.name)}</b>`}</button>`);
  if (btns.length % 2) btns[0] = btns[0].replace('class="btn', 'class="span2 btn');
  const say = kinds ? t('This release comes in more than one kind of build. The usual one works for most players. You can switch later from its right-click menu.') : t('This release comes in more than one build, often one for each region. Pick the one that matches your copy of the game. You can switch later from its right-click menu.');
  modal(`<h2>${esc(tf('Which Version of {name}?', { name: a.name }))}</h2><p>${esc(say)}</p>
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
    if (x.dataset.x === 'forget') { close(); try { await invoke('forget_game', { id }); await keepOut(id); await refreshState(); render(true); } catch (e) { fail(e); } return; }
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

// Set Up for a recomp that only comes as an installer: run it (the player clicks through its own
// windows), find the game where it went, done. Big Picture says to finish it on the desktop.
function installerModal(id) {
  const a = byId(id) || { id, name: (S.library[id] || {}).name || id };
  const g = S.library[id];
  if (!g || !g.installer_setup) {
    // Not downloaded yet (or the download went): Install gets it.
    return startInstall(id);
  }
  const tv = document.body.classList.contains('tv-on');
  const head = (n, title) => `<span class="kick">${esc(tf('Step {n} of 3', { n }))}</span><h2>${esc(title)}</h2>`;
  let st = { stage: 'run' };
  let m = null;
  const paint = () => {
    const box = m && $('.modal', m.s || document);
    let html = '';
    if (st.stage === 'run') html = `${head(1, t('Run the Installer'))}<p>${esc(t('This one comes with its own installer. Run it, and when it asks where to install, keep the folder it suggests.'))}</p><p class="muted mono">${esc(g.path)}</p>${tv ? `<p class="set-note">${esc(t('The installer opens on your desktop. Finish it there, then come back.'))}</p>` : ''}
      <div class="acts grid2"><button type="button" class="btn" data-x="no">${esc(t('Cancel'))}</button><button type="button" class="btn main" data-x="run">${esc(t('Run Installer'))}</button></div>`;
    else if (st.stage === 'wait') html = `${head(1, t('Run the Installer'))}<p>${esc(t('Waiting for the installer to finish…'))}</p><div class="prog"><div class="track busy"><i></i></div></div>
      <div class="acts"><button type="button" class="btn" data-x="stop">${esc(t('Cancel'))}</button></div>`;
    else if (st.stage === 'find') {
      const c = st.check;
      const exes = c && c.exes && c.exes.length ? c.exes : (c && c.exe ? [c.exe] : []);
      html = `${head(2, t('Find the Game'))}<p>${esc(t('Where did it install? Pick the folder with the game in it.'))}</p>
        ${(st.found || []).map((f, i) => `<button type="button" class="btn inst-found" data-x="found" data-i="${i}"><b>${esc(f.exe)}</b><span class="muted">${esc(f.path)}</span></button>`).join('')}
        ${c ? `<p class="muted mono">${esc(c.path)}</p>${exes.length ? `<label class="inst-exe"><span>${esc(t('The game\'s program'))}</span><select class="sel" id="inst-exe">${exes.map((e) => `<option value="${esc(e)}"${e === c.exe ? ' selected' : ''}>${esc(e)}</option>`).join('')}</select></label>` : `<p class="err">${esc(t('No program found in that folder.'))}</p>`}` : ''}
        <div class="acts grid2"><button type="button" class="btn" data-x="pick">${esc(t('Pick a Folder'))}</button><button type="button" class="btn main" data-x="use"${c && exes.length ? '' : ' disabled'}>${esc(t('Use This Folder'))}</button></div>`;
    } else if (st.stage === 'done') html = `${head(3, t('All Set'))}<p>${esc(tf('{name} is in your Library and ready to play.', { name: a.name }))}</p>
      <div class="acts grid2"><button type="button" class="btn" data-x="no">${esc(t('Close'))}</button><button type="button" class="btn main" data-x="play">${esc(t('Play'))}</button></div>`;
    if (box) box.innerHTML = html;
    return html;
  };
  const finished = async (game) => {
    if (game) S.library[id] = game;
    await refreshState().catch(() => {});
    st = { stage: 'done' };
    paint();
    render(true);
  };
  const s0 = modal(paint(), async (x, close) => {
    const k = x.dataset.x;
    if (k === 'run') {
      st = { stage: 'wait' }; paint(); m.s.dataset.busy = '1';
      let r;
      try { r = await invoke('installer_run', { id }); } catch (e) { delete m.s.dataset.busy; st = { stage: 'run' }; paint(); return fail(e); }
      delete m.s.dataset.busy;
      if (r.stopped) { st = { stage: 'run' }; paint(); return; }
      if (r.done) return finished(r.game);
      st = { stage: 'find', found: r.found || [] }; paint(); return;
    }
    if (k === 'stop') return invoke('installer_cancel', { id });
    if (k === 'found') { const f = (st.found || [])[Number(x.dataset.i)]; if (!f) return; try { return finished(await invoke('installer_done', { id, path: f.path, exe: f.exe })); } catch (e) { return fail(e); } }
    if (k === 'pick') {
      const p = await pickFile({ title: t('Where did it install? Pick the folder with the game in it.'), directory: true }).catch(() => null);
      if (!p) return;
      try { st.check = await invoke('installer_check', { id, path: typeof p === 'string' ? p : p.path }); } catch (e) { return fail(e); }
      return paint();
    }
    if (k === 'use') { const exe = ($('#inst-exe', m.s) || {}).value || (st.check || {}).exe; if (!st.check || !exe) return; try { return finished(await invoke('installer_done', { id, path: st.check.path, exe })); } catch (e) { return fail(e); } }
    if (k === 'play') { close(); return playGame(id); }
  }, t('Set Up'));
  m = { s: s0 };
}

function setupReqModal(id) {
  if (isInstallerApp(byId(id)) || installerDue(id)) return installerModal(id);
  // In Big Picture mode it's a sheet made for the controller.
  if (window.tvSetupReq && window.tvSetupReq(id)) return;
  const a = byId(id);
  if (!a) return;
  const inst = S.library[id];
  const steps = (a.setup_steps || []).filter(Boolean);
  const readme = a.setup_readme || (a.repo ? repoReadme(a.repo) : '');
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
  if (S.missing && S.missing.has(parentId(id))) return missingModal(parentId(id));
  // Another game of the same recomp is running: one at a time, since they share one install.
  if (subOf(id) && S.running.has(parentId(id)) && !S.running.has(id)) return toast(tf('{name} is running. Close it first to play another of its games.', { name: (S.library[parentId(id)] || {}).name || '' }), true);
  if (setupDue(byId(id) || { id })) return setupReqModal(id);
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
  const other = [...S.running].find((x) => x !== id && parentId(x) !== parentId(id) && !subOf(x));
  const go2 = async (replace) => {
    try { await invoke('play', { id, replace, bigPicture: document.body.classList.contains('tv-on'), program: program || null }); }
    catch (err) {
      const m = String(err && err.message ? err.message : err).match(/^NEED_COPY\|([^|]*)(?:\|(.*))?$/);
      // The copy picked before didn't have what the game reads: the setup says which files.
      if (m && m[2]) S.setupNote = { id, text: tf('The copy you picked doesn\'t have {files}. Pick the folder that holds them.', { files: m[2] }) };
      if (m || /^NEED_UPDATE\|/.test(String(err && err.message ? err.message : err))) { delete S.setup[id]; return setupModal(id); }
      // It couldn't tell which program is the game (every one looks like a tool): the player says.
      if (/^PICK_EXE\|/.test(String(err && err.message ? err.message : err))) return askProgram(id);
      if (!blocked(id, err)) fail(err);
      if (/program/i.test(String(err))) { S.nextTab = 'installed'; go('game', id); }
    }
  };
  if (!other) return go2(false);
  const on = S.library[other] || { name: other }, me = S.library[id] || { name: id };
  modal(`<h2>${esc(tf('Close {name}?', { name: on.name }))}</h2><p>${esc(tf('{name} is still running. The launcher closes it and starts {other}. Anything you haven\'t saved in it is lost.', { name: on.name, other: me.name }))}</p>
    <div class="acts"><button type="button" class="btn" data-x="no">${esc(t('Keep playing it'))}</button><button type="button" class="btn" data-x="both">${esc(t('Play Both'))}</button><button type="button" class="btn main" data-x="yes">${esc(t('Close it and play'))}</button></div>`, (x, close) => { if (x.dataset.x === 'yes') { close(); go2(true); } if (x.dataset.x === 'both') { close(); go2(false); } }, t('Switch games'));
}

async function askProgram(id) {
  const g = S.library[id] || {};
  let list = [];
  try { list = ((await invoke('game_programs', { id })) || []).filter((p) => !/unins/i.test(p)).slice(0, 8); } catch (e) { /* none */ }
  const sep = String(g.path || '').includes('\\') ? '\\' : '/';
  const full = list.map((p) => `${String(g.path).replace(/[\\/]$/, '')}${sep}${p}`);
  if (!full.length) { S.nextTab = 'installed'; go('game', id); return toast(t('The launcher couldn\'t tell which program starts the game. Pick it in the game\'s settings.'), true); }
  return confirmProgram(id, (libApp(g) || g).name || id, full, true);
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
    if (x.dataset.x === 'gh') invoke('open_url', { url: reportUrl(a) }).catch(fail);
    if (x.dataset.x === 'folder') invoke('open_folder', { id }).catch(fail);
  }, t('Blocked by Windows Security'));
  return true;
}
// Windows' own words for a file it wouldn't let go of (in the PC's language): said plainly.
const cleanMsg = (m) => {
  const s = String(m || '').replace(/^BLOCKED\|[^|]*\|/, '');
  if (S.os !== 'windows') return t(s);
  if (/\(os error 32\)/.test(s)) return t('A file in its folder is in use. Close the game and anything that has its folder open, then try again.');
  if (/\(os error 5\)/.test(s)) return t('Windows didn\'t let the launcher change a file in its folder. Close the game, check that Windows Security isn\'t blocking it (Ransomware protection), then try again.');
  return t(s);
};

// The last update from inside the launcher didn't go in: its installer, run the usual way, shows
// what stopped it (an antivirus, a folder Windows protects), or it's downloaded again.
function updateFailedModal(f) {
  if (document.body.classList.contains('tv-on') || $('.scrim')) { setTimeout(() => updateFailedModal(f), 4000); return; }
  // Twice from the same file: it's the file, not the PC. It isn't offered again; a fixed one is.
  if (f.bad) {
    S.launcherUpd = null; renderSide();
    modal(`<h2>${esc(t('This Update Looks Wrong'))}</h2><p>${esc(tf('RecompLauncher {to} didn\'t install twice, so its update file looks wrong. It won\'t be offered again, and a fixed one shows up on its own once it\'s out. Nothing on your PC needs changing.', { to: verName(f.to) }))}</p>
    <div class="acts"><button type="button" class="btn main" data-x="no">${esc(t('OK'))}</button></div>`, () => {}, t('This Update Looks Wrong'));
    return;
  }
  // On a Mac there's no installer: the new app was put in place, didn't start, and this one was
  // put back. Downloading it again is the only thing to offer.
  if (S.os === 'macos') {
    modal(`<h2>${esc(t('The Update Didn\'t Start'))}</h2><p>${esc(tf('RecompLauncher {to} was put in place but didn\'t start, so {now} was put back. You can try the update again, or download it from recomplauncher.com.', { to: verName(f.to), now: verName(S.version) }))}</p>
    <div class="acts grid2"><button type="button" class="btn" data-x="no">${esc(t('Not Now'))}</button><button type="button" class="btn main" data-x="go">${esc(t('Try Again'))}</button></div>`, (x, close) => {
      if (x.dataset.x !== 'go') return;
      close();
      selfUpdate();
    }, t('The Update Didn\'t Start'));
    return;
  }
  // On Linux there's no installer: a helper swaps the AppImage once the launcher closes. This is
  // still the old version, so the swap didn't happen or the new one didn't start and was put back.
  if (S.os === 'linux') {
    modal(`<h2>${esc(t('The Update Didn\'t Start'))}</h2><p>${esc(tf('RecompLauncher {to} didn\'t go in or didn\'t start, so this is still {now}. You can try the update again, or download it from recomplauncher.com.', { to: verName(f.to), now: verName(S.version) }))}</p>
    <div class="acts grid2"><button type="button" class="btn" data-x="no">${esc(t('Not Now'))}</button><button type="button" class="btn main" data-x="go">${esc(t('Try Again'))}</button></div>`, (x, close) => {
      if (x.dataset.x !== 'go') return;
      close();
      selfUpdate();
    }, t('The Update Didn\'t Start'));
    return;
  }
  modal(`<h2>${esc(t('The Update Didn\'t Install'))}</h2><p>${esc(tf('RecompLauncher {to} was downloaded, but it didn\'t install, so this is still {now}. Run its installer the usual way to see what\'s stopping it, like an antivirus or a folder Windows protects.', { to: verName(f.to), now: verName(S.version) }))}</p>
    <div class="acts grid2"><button type="button" class="btn" data-x="no">${esc(t('Not Now'))}</button><button type="button" class="btn main" data-x="go">${esc(f.there ? t('Run the Installer') : t('Download It Again'))}</button></div>`, async (x, close) => {
    if (x.dataset.x !== 'go') return;
    close();
    if (!f.there) return selfUpdate();
    try { await invoke('update_retry'); } catch (e) { fail(e); }
  }, t('The Update Didn\'t Install'));
}
async function selfUpdate() {
  if (S.selfUpdating) return;
  S.selfUpdating = true;
  toast(t('Downloading the new RecompLauncher…'));
  try {
    if (S.os === 'linux') await invoke('update_launcher', { bigPicture: document.body.classList.contains('tv-on') });
    else await invoke('update_launcher');
  } catch (e) { if (!blocked('', e) && !cantReplace(e)) fail(e); lookForLauncher(); }
  finally { S.selfUpdating = false; }
}
// The AppImage sits in a folder this account can't change, so it can't update itself in place.
// Say where and why, and offer the new AppImage to download instead of a bare error.
function cantReplace(err) {
  const m = String(err && err.message ? err.message : err).match(/^CANT_REPLACE\|([^]*)$/);
  if (!m) return false;
  const url = (S.launcherUpd && S.launcherUpd.url) || `${S.site}/download`;
  const title = t('RecompLauncher Can\'t Update Itself Here');
  modal(`<h2>${esc(title)}</h2><p>${esc(tf('It can\'t replace itself in {folder}, because this account can\'t change that folder. Move RecompLauncher to a folder you own, like Applications in your home folder, or download the new version and use that one.', { folder: m[1] }))}</p>
    <div class="acts grid2"><button type="button" class="btn" data-x="no">${esc(t('Not Now'))}</button><button type="button" class="btn main" data-x="go">${esc(t('Download It'))}</button></div>`, (x, close) => {
    if (x.dataset.x !== 'go') return;
    close();
    invoke('open_url', { url }).catch(fail);
  }, title);
  return true;
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

// Settings → Keep Steam in Sync. Steam only reads its games when it starts, so the launcher writes
// while Steam is closed (it checks every couple of minutes); Sync Now restarts Steam to do it now.
window.steamSync = steamSync;
async function steamSync(restart) {
  let r;
  try { r = await invoke('steam_sync_now', { restart }); } catch (e) { return fail(e); }
  S.steamSyncState = undefined;
  if (r === 'steam_running') {
    const v = await (window.ask || ask)(t('Restart Steam?'), `<p>${esc(t('Steam only picks up games when it starts. The launcher closes Steam, brings it in line with your Library, and opens Steam again. Or leave it open: the changes go in the next time Steam is closed.'))}</p>`,
      [{ label: t('Later'), v: null }, { label: t('Restart Steam'), v: 'yes', main: true }]);
    if (v === 'yes') { toast(t('Bringing Steam in line…')); return steamSync(true); }
  } else if (r === 'in_sync') {
    toast(t('Steam already has all your installed games.'));
  } else if (String(r).startsWith('synced:')) {
    const [, a, d] = String(r).split(':');
    toast(tf('Steam is in sync: {a} added, {d} removed.', { a: Number(a) || 0, d: Number(d) || 0 }));
  }
  if (S.view === 'settings') render(true);
}

// What's waiting to go in or out of Steam, for the setting's line. Looked up again after a sync
// or turning the setting on (false: being looked up).
function steamSyncLine() {
  const st = S.steamSyncState;
  if (st === undefined) { S.steamSyncState = false; invoke('steam_sync_state').then((v) => { S.steamSyncState = v; if (S.view === 'settings' && S.setTab === 'library') setSection('library', false); }).catch(() => {}); }
  if (!st) return t('Every game that\'s installed and set up, with its art.');
  const n = (st.add || 0) + (st.remove || 0);
  if (n) return tn(n, '1 change waits for Steam to close. Sync Now restarts Steam to put it in.', '{n} changes wait for Steam to close. Sync Now restarts Steam to put them in.');
  if (st.collection) return t('The RecompLauncher collection goes in the next time Steam is closed.');
  return tn(st.games || 0, 'Steam has your 1 installed game.', 'Steam has all {n} of your installed games.');
}

function steamSyncGroup(group, row, sw) {
  const on = !!S.settings.steam_sync;
  if (!on) S.steamSyncState = undefined;
  const main = row(t('Keep Steam in Sync'), t('Every game that\'s installed and set up goes in your Steam library with its art, in a RecompLauncher collection, and leaves it when you uninstall it. Games you added to Steam yourself stay. Turning it off leaves Steam as it is.'), sw('steam_sync', on, t('Keep Steam in Sync')));
  const now = on ? row(t('Sync Now'), steamSyncLine(), `<button type="button" class="btn sm" data-act="steam-sync">${esc(t('Sync Now'))}</button>`) : '';
  return group(t('Steam'), main + now);
}

// ---------- screenshots ----------
async function loadAllShots() {
  try { S.allShots = (await invoke('all_screenshots')) || []; } catch (e) { S.allShots = []; }
  if (S.view === 'shots') render(true);
}
const shotList = () => (S.allShots || []).filter((x) => !S.shotsGame || x.id === S.shotsGame);
const when = (ts) => new Date(ts * 1000).toLocaleString(curLocale(), { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' });

// Achievements: the page itself is in achievements.js (testers, and the alpha page for everyone else).
function pageAchievements() { return window.achPage ? window.achPage() : ''; }

// Coming Soon: every recomp the catalog announces before its first release, newest first. Not only
// this PC's: a game that's coming tells the player what to look forward to, whatever it runs on.
const soonList = () => oneEach(visible().filter((a) => a.soon).sort(byNew));
window.soonList = soonList;
const CAL_CLOCK = '<path d="M11.8 21H5a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2h12a2 2 0 0 1 2 2v4"/><path d="M15 3v4M7 3v4M3 11h16"/><circle cx="18" cy="18" r="4"/><path d="M18 16.5V18l1 1"/>';
function pageSoon() {
  if (!S.cat) return `<div class="loading">${esc(t('Loading…'))}</div>`;
  const list = soonList();
  const req = `<button type="button" class="btn main" data-act="recomp-req">${PLUS}${esc(t('Request a Recomp'))}</button>`;
  // The count sits beside the title, small, where Discover puts its own above the grid.
  const count = list.length ? `<span class="count-line soon-n">${esc(tn(list.length, '1 recomp on the way', '{n} recomps on the way'))}</span>` : '';
  return `<div class="page">${offlineNote()}<div class="head"><div><div class="soon-title"><h1>${esc(t('Coming Soon'))}</h1>${count}</div><p>${esc(t('Recomps on the way. Press Notify Me on one and you get an email the day it comes out.'))}</p></div>${req}</div>
    ${list.length ? `<div class="grid" data-n="${list.length}">${list.map((a) => tile(a)).join('')}</div>`
      : `<div class="empty"><svg width="44" height="44" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${CAL_CLOCK}</svg><h2>${esc(t('Nothing announced right now'))}</h2><p>${esc(t('Know of a recomp the launcher should have? Press Request a Recomp to ask for it.'))}</p></div>`}
  </div>`;
}

// Request a recomp: the player picks GitHub or GitLab and types the developer and project
// ("developer/project"), with a note, and it goes to recomplauncher.com for a look. A full link
// pasted into the box works too and picks its site. One the catalog already lists just opens.
const REQ_FORGES = [['github', 'GitHub'], ['gitlab', 'GitLab']];
const reqLink = (forge, typed) => {
  let s = String(typed || '').replace(/\s+/g, '');
  const full = s.match(/^(?:https?:\/\/)?(?:www\.)?(github|gitlab)\.com\/(.*)$/i);
  if (full) { forge = full[1].toLowerCase(); s = full[2]; }
  const parts = s.replace(/[?#].*$/, '').replace(/\.git\/?$/i, '').split('/').filter(Boolean);
  // GitHub is always developer/project; GitLab can have groups in between.
  const ok = parts.length >= 2 && (forge === 'gitlab' || parts.length === 2) && parts.every((x) => /^[\w.-]+$/.test(x) && !/^\.+$/.test(x));
  return { forge, url: ok ? `https://${forge}.com/${parts.join('/')}` : '' };
};
window.reqLink = reqLink;
window.REQ_FORGES = REQ_FORGES;
const sameLink = (x, y) => String(x || '').replace(/\/+$/, '').toLowerCase() === String(y || '').replace(/\/+$/, '').toLowerCase();
function listedFor(link) {
  const full = /^[a-z]+:\/\//i.test(link) ? link : 'https://' + link;
  return all().find((a) => a.repo && sameLink(repoHome(a.repo), full)) || null;
}
window.listedFor = listedFor;
function requestModal() {
  let forge = 'github';
  const m = modal(`<h2>${esc(t('Request a Recomp'))}</h2><p>${esc(t('Know of a recomp the launcher doesn\'t have? Pick where its project is, type the developer and the project, and it goes to RecompLauncher for a look.'))}</p>
    <div class="seg rq-forge" role="radiogroup" aria-label="${esc(t('Where the project is'))}">${REQ_FORGES.map(([k, n]) => `<button type="button" role="radio" aria-checked="${k === forge}" data-x="forge" data-forge="${k}">${esc(n)}</button>`).join('')}</div>
    <label class="field rq-path"><span class="rq-host">github.com/</span><input id="rq-url" type="text" placeholder="${esc(t('developer/project'))}" spellcheck="false" autocomplete="off" maxlength="200" aria-label="${esc(t('Developer and project'))}"></label>
    <textarea class="field" id="rq-note" rows="3" maxlength="500" placeholder="${esc(t('Anything to add? Which game it is, or why you\'d like it (optional)'))}" aria-label="${esc(t('Note'))}"></textarea>
    <p class="err" id="rq-err" hidden></p>
    <div class="acts grid2"><button type="button" class="btn" data-x="no">${esc(t('Cancel'))}</button><button type="button" class="btn main" data-x="yes">${esc(t('Send Request'))}</button></div>`, async (x, close) => {
    if (x.dataset.forge) return setForge(x.dataset.forge);
    if (x.dataset.x !== 'yes') return;
    const typed = $('#rq-url', m).value.trim(), note = $('#rq-note', m).value.trim(), err = $('#rq-err', m);
    if (!typed) return $('#rq-url', m).focus();
    const r = reqLink(forge, typed);
    setForge(r.forge);
    if (!r.url) { err.textContent = t('Type the developer and the project, like developer/project.'); err.hidden = false; return $('#rq-url', m).focus(); }
    const url = r.url;
    const known = listedFor(url);
    if (known) { close(); toast(tf('{name} is already in the launcher. Here it is.', { name: known.name })); return go('game', known.id); }
    x.disabled = true; err.hidden = true;
    const was = x.textContent; x.textContent = t('Sending…');
    try {
      await invoke('recomp_request', { url, note: note || null });
      close();
      toast(t('Request sent. Thanks! It\'ll be looked at soon.'));
    } catch (e) {
      err.textContent = t(String(e && e.message ? e.message : e)); err.hidden = false;
      x.disabled = false; x.textContent = was;
    }
  }, t('Request a Recomp'));
  const setForge = (k) => {
    forge = k;
    m.querySelectorAll('[data-forge]').forEach((b) => b.setAttribute('aria-checked', String(b.dataset.forge === k)));
    $('.rq-host', m).textContent = `${k}.com/`;
  };
  const f = $('#rq-url', m); f.focus();
  f.addEventListener('keydown', (e) => { if (e.key === 'Enter') $('[data-x="yes"]', m).click(); });
  // A whole link pasted in: its site is picked and only developer/project stays in the box.
  f.addEventListener('input', () => { const full = f.value.trim().match(/^(?:https?:\/\/)?(?:www\.)?(github|gitlab)\.com\/(.+)$/i); if (full) { setForge(full[1].toLowerCase()); f.value = full[2].replace(/\.git\/?$/i, '').replace(/\/+$/, ''); } });
}
window.requestModal = requestModal;

function pageShots() {
  if (!S.allShots) return `<div class="loading">${esc(t('Loading your screenshots…'))}</div>`;
  const all = S.allShots, list = shotList();
  const games = uniq(all.map((x) => x.id)).map((id) => ({ id, name: (all.find((x) => x.id === id) || {}).game, n: all.filter((x) => x.id === id).length }));
  if (S.shotsGame && !games.some((g) => g.id === S.shotsGame) && S.library[S.shotsGame]) games.push({ id: S.shotsGame, name: S.library[S.shotsGame].name, n: 0 });
  return `<div class="page"><div class="head"><div><h1>${esc(t('Screenshots'))}</h1><p>${esc(t('F12, or View + RB on a controller, while you play. They\'re kept in Pictures\\RecompLauncher.'))}</p></div>
      ${all.length ? `<button type="button" class="btn" data-act="shots-root">${esc(t('Open the folder'))}</button>` : ''}</div>
    ${games.length ? `<div class="chips"><button type="button" class="chip${!S.shotsGame ? ' on' : ''}" data-act="shots-game" data-g="">${esc(t('All'))} ${fmtNum(all.length)}</button>${games.map((g) => `<button type="button" class="chip${S.shotsGame === g.id ? ' on' : ''}" data-act="shots-game" data-g="${esc(g.id)}">${esc(g.name)} ${fmtNum(g.n)}</button>`).join('')}</div>` : ''}
    ${list.length ? `<div class="shotgrid">${list.map((x, i) => `<button type="button" class="shot" data-act="shot-open" data-i="${i}" data-path="${esc(x.path)}"><img src="${esc(fileSrc(x.path))}" alt="" loading="lazy"><span><b>${esc(x.game)}</b><small>${esc(when(x.time))}</small></span></button>`).join('')}</div>`
      : `<div class="empty"><svg width="44" height="44" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${MI.shot}</svg><h2>${esc(t('No screenshots yet'))}</h2><p>${esc(S.os === 'macos' ? t('In-game screenshot capture is not available on Mac yet.') : t('While a game is running, press F12 or View + RB. They show up here, ready to share.'))}</p></div>`}
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
// A new RecompLauncher, announced while the launcher is open: looked for when it opens (an answer
// from the last hour is reused); after that the core looks every six hours or so, and sooner when
// the website pushes a new build ('launcher-update' in more.js). Each new version is announced once.
const LL = { told: '' };
async function lookForLauncher(atStart) {
  let v = null;
  try { v = await invoke('launcher_update', { force: false }); } catch { return; }
  // A file that turned out wrong isn't offered any more, so the banner goes too.
  S.launcherUpd = v && v.version ? v : null;
  if (v && v.version && v.version !== LL.told) {
    LL.told = v.version;
    // When the launcher opens: a screen of its own (unless it was opened for something else).
    if (atStart && startUpdateDue(v)) setTimeout(() => { if (startUpdateDue(v)) updateStart(v); else updateToast(v); }, 1200);
    else updateToast(v);
  }
  renderSide();
}

// An update restart returns to the mode that was on screen.
function startsInTV() { return !S.desktopArg && !!(S.settings.start_in_tv || S.bigArg); }

// The update screen at start: not during first-time setup, in Big Picture, with a game running
// (started from a shortcut) or a window already open (a game link, a Set Up), and not for a
// version the player put off with Later, unless that version is required.
function startUpdateDue(v) {
  if (!v || !v.version || !S.settings.setup_done) return false;
  if (document.body.classList.contains('tv-on') || startsInTV()) return false;
  if ((S.running && S.running.size) || $('.scrim') || $('.upd-start')) return false;
  return !!v.required || (S.settings.ui || {}).upd_later !== v.version;
}
function updateStart(v) {
  const el = document.createElement('div');
  el.className = 'upd-start';
  el.setAttribute('role', 'dialog');
  el.setAttribute('aria-modal', 'true');
  el.setAttribute('aria-labelledby', 'upd-start-h');
  const notes = v.notes ? `<div class="md upd-start-notes">${md(v.notes, true)}</div>` : '';
  const later = v.required ? '' : `<button type="button" class="btn" data-x="later">${esc(t('Later'))}</button>`;
  el.innerHTML = `<div class="upd-start-box"><img class="upd-start-mark" src="img/mark.svg" alt=""><h1 id="upd-start-h">${esc(tf('RecompLauncher {version} is ready', { version: verName(v.version) }))}</h1>
    <p class="muted">${esc(v.required ? tf('You have {version}, which is too old to keep up with the recomps. Update to keep playing. It only takes a moment.', { version: verName(S.version) }) : tf('You have {version}. Updating takes a moment, and RecompLauncher opens again by itself.', { version: verName(S.version) }))}</p>
    ${notes}<div class="acts ${later ? 'grid2' : ''}">${later}<button type="button" class="btn main" data-x="now">${esc(t('Update Now'))}</button></div>
    ${later ? `<p class="muted upd-start-small">${esc(t('Later keeps this version on the Downloads page until a newer one is out.'))}</p>` : ''}</div>`;
  document.body.appendChild(el);
  const close = () => { el.remove(); document.removeEventListener('keydown', key, true); };
  const key = (e) => { if (e.key === 'Escape' && !v.required) { e.preventDefault(); e.stopPropagation(); close(); saveUi({ upd_later: v.version }); } };
  document.addEventListener('keydown', key, true);
  el.addEventListener('click', async (e) => {
    const b = e.target.closest('[data-x]');
    if (!b) return;
    if (b.dataset.x === 'later') { close(); saveUi({ upd_later: v.version }); return; }
    // Update Now: the download's own window takes over; if it doesn't go, this screen goes too, so
    // the launcher stays usable.
    close();
    selfUpdate();
  });
  setTimeout(() => { const b = $('[data-x="now"]', el); if (b) b.focus(); }, 50);
}

// A version too old to keep up (older than the list's "min"): a window, once each time the
// launcher opens, instead of a toast that goes away.
let requiredShown = false;
// Several betas behind (or a whole version): worth a window, not just a toast. Fixes from the
// betas in between are what most crash reports from old versions turn out to need.
function farBehind(cur, next) {
  const p = (x) => { const m = /^(\d+)\.(\d+)\.(\d+)(?:-beta\.(\d+))?/.exec(String(x || '')); return m ? [+m[1], +m[2], +m[3], m[4] == null ? Infinity : +m[4]] : null; };
  const a = p(cur), b = p(next);
  if (!a || !b) return false;
  if (a[0] !== b[0] || a[1] !== b[1]) return true;
  return a[3] !== Infinity && b[3] !== Infinity && b[3] - a[3] >= 2;
}
function updateToast(v) {
  if (v && (v.required || farBehind(S.version, v.version)) && !requiredShown) {
    requiredShown = true;
    modal(`<h2>${esc(t('Time to Update'))}</h2><p>${esc(v.required ? tf('This version of RecompLauncher is too old to keep up with the recomps. Update to {version} to keep playing. It only takes a moment.', { version: verName(v.version) }) : tf('You\'re a few updates behind. {version} has fixes for crashes and setup problems people ran into on this one. It only takes a moment.', { version: verName(v.version) }))}</p>
      <div class="acts grid2"><button type="button" class="btn" data-x="no">${esc(t('Not now'))}</button><button type="button" class="btn main" data-x="yes">${esc(t('Update now'))}</button></div>`, (x, close) => { close(); if (x.dataset.x === 'yes') selfUpdate(); }, t('Time to Update'));
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
    <p>${esc(t('Recompiled and decompiled PC games: find, install, update and play them, straight from each developer\'s own GitHub.'))}</p>
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
  if (!(S.settings.rom_dirs || []).length) { S.roms = []; S.owned = {}; await addOwned(); render(true); return; }
  toast(t('Looking through your folders…'));
  try { S.roms = await invoke('scan_roms'); await loadOwned(); const added = await addOwned(); toast(tn(S.roms.length, 'Found 1 game file.', 'Found {n} game files.') + ' ' + (added ? tn(added, '1 recomp went into your Library.', '{n} recomps went into your Library.') : tf('{n} have recomps.', { n: Object.keys(S.owned).length }))); } catch (e) { fail(e); }
  render(true);
}
// Recomps for games the scan found go into the Library (not installed yet), ready for Easy Setup.
// The ones it put there leave again once their game isn't found any more (its folder was taken
// out of the list, or the files are gone), unless they've been installed since.
// The game folders Dolphin, RPCS3, PCSX2, RetroArch, Cemu and Ryujinx already list: added in one
// go (the player sees which), and looked through like any other game folder.
async function emuFolders(quiet) {
  let found = [];
  try { found = (await invoke('emulator_folders')) || []; } catch (e) { found = []; }
  if (!found.length) {
    // The Flatpak can't read the emulators' settings: their folders are added by hand.
    if (!quiet) toast(S.flatpak ? t('The Flatpak can\'t read your emulators\' settings. Add their game folders with Add a folder.') : t('No emulator game folders found on this PC beyond the ones already added.'));
    return false;
  }
  const v = await ask(t('Add Your Emulators\' Game Folders?'), `<p>${esc(t('These folders are where your emulators keep games. The launcher only reads them.'))}</p><ul class="lacks">${found.map((f) => `<li><b>${esc(f.emu)}</b> · <span class="path">${esc(f.path)}</span></li>`).join('')}</ul>`,
    [{ label: t('Not Now'), icon: 'chevL', v: null }, { label: tn(found.length, 'Add 1 Folder', 'Add {n} Folders'), icon: 'plus', v: 'add', main: true }]);
  if (v !== 'add') return false;
  await saveSettings({ rom_dirs: uniq([...(S.settings.rom_dirs || []), ...found.map((f) => f.path)]) });
  await scanRoms();
  return true;
}
async function addOwned() {
  const s = Object.assign({}, saved()), now = Math.floor(Date.now() / 1000);
  const ui = S.settings.ui || {};
  const mine = new Set(ui.found_saved || []);
  // Ones the player took out of the Library (or uninstalled) stay out.
  const skip = new Set(ui.found_skip || []);
  const titleOf = (id) => (byId(id) || {}).title || '';
  // A game with one of its versions installed: the others a look put in go again.
  const installedTitles = new Set(Object.keys(S.library).map(titleOf).filter(Boolean));
  // Only from a look that worked (offline, nothing is known either way).
  const known = S.ownedOk !== false || !(S.settings.rom_dirs || []).length;
  const gone = [...mine].filter((id) => (known && !(S.owned || {})[id]) || S.library[id] || skip.has(id) || (installedTitles.has(titleOf(id)) && !S.library[id]));
  for (const id of gone) { mine.delete(id); if (!S.library[id]) delete s[id]; }
  // One version of each game, and none of a game the player already has a version of (installed
  // or in the Library): the recommended version when it's among those found.
  const have = new Set([...Object.keys(S.library), ...Object.keys(s)].map(titleOf).filter(Boolean));
  const rank = (id) => { const g = index().titles.get(titleOf(id)); return g ? (g.pick === id ? -1 : g.every.findIndex((a) => a.id === id)) : 0; };
  const found = Object.keys(S.owned || {}).filter((id) => !S.library[id] && !s[id] && !skip.has(id) && byId(id) && !byId(id).soon && forOS(byId(id))).sort((x, y) => rank(x) - rank(y));
  const add = [];
  for (const id of found) {
    const ti = titleOf(id);
    if (ti && have.has(ti)) continue;
    if (ti) have.add(ti);
    add.push(id);
  }
  for (const id of add) { s[id] = now; mine.add(id); }
  if (add.length || gone.length) await saveUi({ saved: s, found_saved: [...mine] }, true);
  return add.length;
}
window.addOwned = addOwned;
// Taken out of the Library by the player (removed, or uninstalled): a look at their games doesn't
// put it back.
async function keepOut(id) {
  const skip = new Set(((S.settings.ui || {}).found_skip) || []);
  if (skip.has(id)) return;
  skip.add(id);
  await saveUi({ found_skip: [...skip] }, true);
}
window.keepOut = keepOut;

// Catalog games the launcher sets up itself (copy and settings).
async function loadEasy(quiet) {
  try { S.easy = new Set(await invoke('easy_setup')); } catch (e) { S.easy = new Set(); }
  if (!quiet) render(true);
}
async function loadOwned() {
  try { S.owned = await invoke('rom_matches'); S.ownedOk = true; } catch (e) { S.owned = {}; S.ownedOk = false; }
  S.copyFiles = {};
  S.owned = ownedHere(S.owned);
}
// Whether a file sits inside a folder. A bare prefix match would count D:\Dumps2\a.iso under D:\Dumps.
function inFolder(file, dir) {
  const norm = (p) => String(p || '').replace(/\\/g, '/').replace(/\/+$/, '');
  const folder = norm(dir);
  return norm(file).startsWith(folder + '/');
}
// Files are not games: a folder holds updates, DLC and extra discs too, and the Library keeps one
// entry per game. Both numbers are shown so the folder count doesn't read as a game count.
function folderCounts(dir) {
  const files = S.roms.filter((r) => inFolder(r.path, dir)).length;
  const recomps = Object.values(S.owned || {}).filter((o) => inFolder(o.path, dir)).length;
  return tf('Files found: {files} · With recomps: {recomps}', { files, recomps });
}
// Only recomps this PC can play count as found (a Windows-only one on Linux without Proton doesn't).
function ownedHere(o) {
  o = Object.assign({}, o || {});
  const ui = (S.settings || {}).ui || {}, drop = ui.found_drop || {};
  // Files the player said aren't theirs don't count as found.
  for (const [id, list] of Object.entries(drop)) if (o[id] && (list || []).includes(o[id].path)) delete o[id];
  // The copy the player chose instead, when the one found wasn't right (another console's, say),
  // or when none was found.
  for (const [id, path] of Object.entries(ui.found_fix || {})) {
    if (!path || (drop[id] || []).includes(path)) continue;
    o[id] = Object.assign({ system: '', title: '', size: 0 }, o[id] || {}, { path, region: o[id] && o[id].path === path ? o[id].region : '' });
  }
  if (!S.cat || !S.cat.apps.length) return o;
  return Object.fromEntries(Object.entries(o).filter(([id]) => { const a = byId(id); return !a || forOS(a); }));
}
// A long folder, shortened in the middle: C:\Games\…\Switch\Zelda.
const midShort = (p, n = 46) => { p = String(p || ''); if (p.length <= n) return p; const h = Math.floor((n - 1) / 2); return p.slice(0, h) + '…' + p.slice(p.length - (n - 1 - h)); };
// Everything found for a game, grouped: the game (each disc), its update, extra content. Each one
// can be dropped on its own, or another picked by hand.
async function copiesModal(id) {
  const a = byId(id) || { id, name: id };
  const draw = () => {
    const c = (S.copyFiles || {})[id] || { files: [], missing_discs: [] };
    const groups = [['game', t('Game')], ['update', t('Update')], ['dlc', t('DLC')]];
    const row = (f) => `<div class="mh-row copy-row"><div class="mh-k"><b>${f.disc ? `<span class="tag">${esc(tf('Disc {n}', { n: f.disc }))}</span> ` : ''}${esc(f.name)}</b>
      <span class="path" title="${esc(f.path)}">${esc([midShort(f.folder), f.size ? fmtB(f.size) : '', f.region].filter(Boolean).join(' · '))}</span></div>
      <span class="copy-tags">${f.verified ? `<span class="tag ok">${esc(t('Verified'))}</span>` : ''}${f.picked ? `<span class="tag">${esc(t('In Use'))}</span>` : ''}</span>
      <button type="button" class="btn sm" data-x="drop" data-path="${esc(f.path)}">${esc(t('Not This One'))}</button></div>`;
    const body = groups.map(([k, name]) => { const list = c.files.filter((f) => f.kind === k); return list.length ? `<h4 class="copy-h">${esc(name)}</h4>${list.map(row).join('')}` : ''; }).join('');
    const gap = c.missing_discs.length ? `<p class="warn-line">${esc(tn(c.missing_discs.length, 'Disc {list} is missing.', 'Discs {list} are missing.').replace('{list}', c.missing_discs.join(', ')))}</p>` : '';
    return `<h2>${esc(t('Your Copy of the Game'))}</h2><p class="muted">${esc(tf('What the launcher found for {name} in your game folders.', { name: a.name }))}</p>
      ${gap}<div class="mh copy-list">${body || `<p class="muted">${esc(t('Not found'))}</p>`}</div>
      <div class="acts grid2"><button type="button" class="btn" data-x="another">${esc(t('Choose Another…'))}</button><button type="button" class="btn main" data-x="no">${esc(t('Close'))}</button></div>`;
  };
  await loadCopies(id);
  const scrim = modal(draw(), async (x, close) => {
    const box = $('.modal', scrim);
    if (x.dataset.x === 'drop') {
      const path = x.dataset.path;
      const ui = S.settings.ui || {};
      const drop = Object.assign({}, ui.found_drop || {});
      drop[id] = uniq([...(drop[id] || []), path]);
      const fix = Object.assign({}, ui.found_fix || {});
      // The one in use goes: the next disc or game file found takes its place.
      const own = S.owned[id];
      if (own && own.path === path) {
        const next = (((S.copyFiles || {})[id] || {}).files || []).find((f) => f.kind === 'game' && f.path !== path && !drop[id].includes(f.path));
        if (next) fix[id] = next.path; else delete fix[id];
      }
      await saveUi({ found_drop: drop, found_fix: fix }, true);
      await loadOwned();
      await loadCopies(id);
      box.innerHTML = draw();
      render(true);
      return toast(tf('{file} isn\'t used for this game anymore.', { file: baseName(path) }));
    }
    if (x.dataset.x === 'another') {
      const p = await pickFile(copyPickOpts(id)).catch(fail);
      const path = p && (typeof p === 'string' ? p : p.path);
      if (!path) return;
      const ui = S.settings.ui || {};
      const drop = Object.assign({}, ui.found_drop || {});
      if (drop[id]) drop[id] = drop[id].filter((d) => d !== path);
      await saveUi({ found_fix: Object.assign({}, ui.found_fix || {}, { [id]: path }), found_drop: drop }, true);
      await loadOwned();
      await loadCopies(id);
      box.innerHTML = draw();
      render(true);
      toast(tf('It uses {file} from now on.', { file: baseName(path) }));
    }
  }, t('Your Copy of the Game'));
}

async function fixFound(id) {
  const p = await pickFile(copyPickOpts(id)).catch(fail);
  const path = p && (typeof p === 'string' ? p : p.path);
  if (!path) return;
  const drop = Object.assign({}, ((S.settings.ui || {}).found_drop) || {});
  if (drop[id]) drop[id] = drop[id].filter((d) => d !== path);
  await saveUi({ found_fix: Object.assign({}, ((S.settings.ui || {}).found_fix) || {}, { [id]: path }), found_drop: drop }, true);
  await loadOwned();
  render(true);
  toast(tf('It uses {file} from now on.', { file: baseName(path) }));
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
    0xC0000006: N_('Windows couldn\'t read it from its drive'),
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
    9009: N_('a command it runs isn\'t installed'),
  };
  return M[u] ? t(M[u]) : '';
}
// Buttons in two even columns: an odd one out (the main one) goes on top across both.
function evenActs(btns) {
  btns = btns.filter(Boolean);
  if (btns.length % 2) { let i = btns.findIndex((x) => /class="btn main"/.test(x)); if (i < 0) i = 0; const [m] = btns.splice(i, 1); btns.unshift(m.replace('class="btn', 'class="span2 btn')); }
  return btns.join('');
}
// A new issue on RecompLauncher's GitHub for a crash, filled in: the versions, the PC, the game and
// the end of its log. Home folders become "~" and the excerpt loses lines from the top until the
// address fits in 8 KB, which is about what GitHub and browsers take.
const ghCrashUrl = (c) => {
  const home = (s) => String(s || '').replace(/([A-Za-z]:\\Users\\|\/home\/|\/Users\/)[^\\/\s]+/g, '~');
  const reason = String(c.reason || '').replace(/\s+/g, ' ').trim();
  const title = `Crash: ${reason.length > 80 ? `${reason.slice(0, 79)}…` : reason}`;
  const head = [c.note ? `**What happened:** ${c.note}` : '**What happened:** ', '',
    `- RecompLauncher: ${c.version}`, `- System: ${c.os}`, `- Graphics card: ${c.gpu || 'unknown'}`,
    ...(c.game ? [`- Game: ${c.game}`] : []), ...(c.exit ? [`- ${c.exit}`] : [])];
  const ask = 'Please attach the full log too: drag the file into this box. Open its folder in the crash window shows where it is.';
  const base = 'https://github.com/elripalda/RecompLauncher/issues/new';
  let lines = c.file ? home(c.text).split('\n').slice(-60) : [];
  for (;;) {
    const ex = lines.join('\n'), fence = ex.includes('```') ? '````' : '```';
    const body = [...head, ...(c.file ? ['', `Last ${lines.length} lines of ${home(c.file)}:`, `${fence}text`, ex, fence] : []), '', ask].join('\n');
    const url = `${base}?title=${encodeURIComponent(title)}&body=${encodeURIComponent(body)}`;
    if (url.length <= 8000 || !lines.length) return url;
    // One very long line can't go whole: keep its end.
    if (lines.length === 1) lines = lines[0].length > 400 ? [lines[0].slice(-Math.floor(lines[0].length / 2))] : [];
    else lines = lines.slice(1);
  }
};
// What players found for a crash, from the catalog: a game's "fixes", each with the text that shows
// the problem ("when", * for any text, case doesn't matter) and what to do ("say"), optionally only
// for some versions. The first that matches the game's output wins.
const verKey = (v) => String(v || '').trim().toLowerCase().replace(/^v(?=\d)/, '');
function knownFix(a, text, tag) {
  const list = (a && Array.isArray(a.fixes)) ? a.fixes : [];
  const hay = String(text || '').toLowerCase();
  for (const f of list) {
    if (!f || typeof f.when !== 'string' || typeof f.say !== 'string' || !f.when.trim() || !f.say.trim()) continue;
    if (Array.isArray(f.versions) && f.versions.length && !f.versions.some((v) => verKey(v) === verKey(tag))) continue;
    const parts = f.when.toLowerCase().split('*').map((x) => x.trim()).filter(Boolean);
    let at = 0, ok = parts.length > 0;
    for (const part of parts) { const i = hay.indexOf(part, at); if (i < 0) { ok = false; break; } at = i + part.length; }
    if (ok) return f.say.trim();
  }
  return '';
}
// A version the site found crashing for many players ("bad_versions": { version, use, note }).
function badVersion(a, tag) {
  const list = (a && Array.isArray(a.bad_versions)) ? a.bad_versions : [];
  return tag ? list.find((b) => b && verKey(b.version) && verKey(b.version) === verKey(tag)) || null : null;
}
// Updated in the last two weeks and played fine before it (a session of a minute or more): a crash
// now may be the update's, and the version before is one press away.
function updateBroke(g) {
  if (!g || !g.prev_tag || !g.updated_at || verKey(g.prev_tag) === verKey(g.tag)) return false;
  // Never back to a version the site says is crashing.
  if (badVersion(byId(g.id), g.prev_tag)) return false;
  const upd = Number(g.updated_at);
  if (Date.now() / 1000 - upd > 14 * 86400) return false;
  return (g.sessions || []).some((x) => x[0] < upd && x[1] >= 60);
}
async function crashModal(p) {
  (S.lastExit = S.lastExit || {})[p.id] = p.code;
  const g = S.library[p.id] || { name: p.id };
  // A community server that stopped at once: on its first start it writes its settings file and
  // waits for them to be filled in. Not a crash.
  if (isServer(byId(p.id) || { id: p.id }) && p.secs < 10) {
    return toast(t('The server made its settings file (server.toml) in its folder. Fill it in, then start it again.'), false, `${esc(t('The server made its settings file (server.toml) in its folder. Fill it in, then start it again.'))} <a href="#" data-act="folder" data-id="${esc(p.id)}">${esc(t('Open its folder'))}</a>`, 15000);
  }
  // The player said this exit was them closing it (a game that always closes with code 1, say):
  // no crash window for it again.
  if (p.secs >= 5 && (((S.settings.ui || {}).quiet_exits || {})[p.id] || []).includes(Number(p.code))) return;
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
  const gpu = /no Vulkan [\d.]+ device|vkCreateDevice\b[^\n]{0,80}failed|renderer init failed|doesn't implement the VK_KHR_surface|unable to create a d3d12 device|volkInitialize failed|vkCreateInstance failed|VK_ERROR_INCOMPATIBLE_DRIVER|vulkan (?:is )?not (?:supported|available)|does not support vulkan|failed to (?:create|initiali[sz]e) (?:the )?(?:vulkan|d3d12|directx 12)|unable to initiali[sz]e direct3d 12|unable to find (?:any )?devices? that supports? vulkan|\bvk[A-Z]\w+(?: could)? not (?:be )?located|Vulkan [\d.]+ is required,? but only|Required (?:Vulkan )?extensions are missing|unable to create (?:a )?vulkan instance|FeatureNotPresent|ErrorFeatureNotPresent|VK_ERROR_FEATURE_NOT_PRESENT/i.test(said);
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
  // It couldn't make a folder inside its own folder, which isn't somewhere protected: Windows
  // Security's Controlled folder access (ransomware protection) blocks programs it doesn't know.
  const blocked = !noWrite && S.os === 'windows' && /(?:cannot|can't|could not|couldn't|failed to|unable to) create (?:the )?(?:directory|folder)/i.test(said);
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
  // Which files those are: the Visual C++ runtime's, the old DirectX runtime's, or the game's own
  // (then Visual C++ isn't what's missing, however often it's installed).
  let lacking = [];
  if (dll) { try { lacking = (await invoke('missing_files', { id: p.id })) || []; } catch (e) { lacking = []; } }
  const low = lacking.map((d) => String(d).toLowerCase());
  // "Entry point not found": nothing's missing, a DLL is too old (almost always the Visual C++ runtime).
  const entryGone = [-1073741511, 3221225785].includes(Number(p.code));
  const dllVC = dll && (entryGone || !low.length || low.some((d) => /^(msvcp|vcruntime|concrt|vcomp|mfc1|vccorlib)/.test(d)));
  const dllDX = dll && !dllVC && low.some((d) => /^(d3dx9_|d3dx10_|d3dx11_|xinput1_3|x3daudio|xaudio2_7|d3dcompiler_4)/.test(d));
  let dllOwn = dll && !dllVC && !dllDX;
  // "Too old": an old copy the game came with, an old Visual C++, or an old graphics driver.
  let dllf = null;
  if (entryGone && S.os === 'windows') { try { dllf = await invoke('dll_check', { id: p.id }); } catch (e) { /* none */ } }
  const oldCopies = dllf && dllf.old_copies && dllf.old_copies.length ? dllf.old_copies : null;
  const oldVulkan = !oldCopies && dllf && !dllf.vc_old && dllf.vulkan_old;
  // Missing files the game makes itself from the player's copy: setting the copy up brings them.
  let made = [];
  if (dllOwn) { try { made = (await invoke('missing_made', { id: p.id })) || []; } catch (e) { made = []; } }
  const madeOnly = dllOwn && made.length > 0;
  if (madeOnly) dllOwn = false;
  // It didn't understand the player's Launch options.
  const badOpts = !!String(g.args || '').trim() && /unknown (?:option|argument|flag|parameter)|unrecognized (?:option|argument|command.line)|invalid (?:option|argument)|unexpected argument/i.test(said);
  // It printed how it wants to be started (it needs the player's files passed to it).
  const usage = !rootFlag && !badOpts && /^\s*usage:/im.test(said);
  // It printed how it's used and stopped at once: most often one of the game's tools (a converter,
  // a patcher) started instead of the game.
  const toolRan = usage && p.secs < 5;
  // The player's copy isn't the version it's made for.
  // Windows couldn't read the game's program from its drive while it ran (a network folder or a
  // USB drive that dropped out for a moment).
  const inPage = [-1073741818, 3221225478].includes(Number(p.code));
  const vram = /OUT_OF_DEVICE_MEMORY|vmaCreate\w+\(.*failed: -2\b|out of (?:video|device|gpu) memory/i.test(said);
  // Windows ran out of memory for it (RAM and the page file together).
  const oom = !vram && ([-2147024882, 2147942414, -1073741801, 3221225495].includes(Number(p.code)) || /memory commit|commit limit|out of memory|std::bad_alloc|failed to allocate \d+ bytes/i.test(said));
  // Linux: the Proton it ran through lacks a part of Windows this game calls.
  const protonGap = /wine: Call from [0-9A-Fa-f]+ to unimplemented function/i.test(said);
  // It crashed inside a mod (its crash report names a file in a mods folder), not the game.
  const modHit = /["'\s=]([^"'\r\n]*?[\\/]+mods[\\/]+(?:\.cache[\\/]+)?([^"'\\/\r\n]+)[\\/]+[^"'\r\n]*?\.(?:dll|so|dylib))/i.exec(said);
  const modId = modHit ? modHit[2].replace(/\.(?:zip|dll|so)$/i, '') : '';
  const modName = /^(?:com|dev|io|org|net|me|xyz|gg|app)\.[^.]+\./i.test(modId) ? modId.split('.').pop() : modId;
  // Linux: the Proton it ran through is built for another processor (an ARM64 one on a normal PC).
  // umu-launcher's Steam Linux Runtime didn't download properly (a checksum that didn't match, or
  // parts missing), so nothing it starts can run.
  const umuRt = S.os === 'linux' && /Digest mismatch|Runtime Platform missing or download incomplete|_v2-entry-point \(umu\) cannot be found|umu-shim (?:not found|cannot be found|is missing)/i.test(said);
  // A Proton build without its starting Windows setup (some distribution packages): it can't make
  // the game's prefix at all. GE-Proton comes complete.
  const pfxGone = S.os === 'linux' && /Default prefix is missing/i.test(said);
  const armProton = pfxGone || S.os === 'linux' && (/steamrt[\w.-]*arm[\w-]* validation failed/i.test(said) || (/Exec format error/i.test(said) && /\.(exe|bat|cmd)$/i.test(String(g.exe || ''))));
  // Linux: a native build made for a newer Linux than this one (glibc).
  const glibc = S.os === 'linux' && /version [`'"]?GLIBC_[\w.]+[`'"]? not found/i.test(said);
  // Linux: a library the game needs isn't on this PC.
  const noLib = /error while loading shared libraries: ([^\s:]+)/i.exec(said);
  // It needs Python (its setup or a build step runs it), and there isn't one.
  const python = /could not find python|python was not found|python3? is not recognized|python3?: (?:command )?not found|no python (?:interpreter|installation)|requires python ?3|install python ?3/i.test(said);
  // A script it runs called a program that isn't installed (cmd's code 9009).
  const noCmd = !python && (Number(p.code) === 9009 || /is not recognized as an internal or external command/i.test(said));
  // Vulkan's loader couldn't use the graphics card: an old driver, or an overlay's Vulkan add-on
  // getting in the way.
  const vkLoader = !gpu && /\[vulkan loader\] error|vkGetPhysicalDevice\w*: invalid physicalDevice|vkEnumeratePhysicalDevices\w*: invalid|Invalid physicalDevice/i.test(said);
  const wrongDisc = /no game archives|not the right disc|wrong disc/i.test(said);
  // Windows itself stopped the program: Smart App Control, or an app control policy.
  const sac = [-1058471934, 3236495362].includes(Number(p.code)) || /blocked by (?:smart app control|your organization|app control)|application control policy has blocked|0x800711C7/i.test(said);
  // The release has a program for each version of the game, and this one isn't for the player's.
  const otherBuild = /use the build that matches|recompiled from a different version of the game executable/i.test(said);
  const wrongCopy = !armProton && /wrong rom|rom is not supported|rom (?:file )?is truncated|truncated or invalid|invalid rom|cartridge id at|bad n64 magic|validation failed|isn't the (?:pal|ntsc|usa|us|european|japanese|japan|europe)\b|failed validation|re-dump|verify your file matches|hash mismatch|doesn't match any known|not a supported (?:dump|rom|version)|unsupported (?:rom|dump|version|revision)|wrong (?:region|revision)|bad dump|country byte|rejected: .{0,40}expected|region mismatch|sha1 [0-9a-f]{6,}.{0,30}another release|another release of (?:the|this) game/i.test(said);
  // It says to run its own setup first: offer the setup program that came with it.
  // When it names the program ("Run theatrhythm-setup.exe"), that one; otherwise one that looks like a setup.
  let setupExe = '';
  const named = said.match(/\brun\s+["'`]?([\w .()-]+\.(?:exe|bat|cmd))\b/i);
  if (named || /run (?:the |its )?setup|setup to unpack|setup first|\brun [\w.-]*setup[\w.-]*/i.test(said)) {
    try {
      const progs = (await invoke('game_programs', { id: p.id })) || [];
      const base = (x) => x.split(/[\\/]/).pop().toLowerCase();
      setupExe = (named && progs.find((x) => base(x) === named[1].trim().toLowerCase()))
        || progs.find((x) => { const n = base(x); return /setup|install|unpack|extract/i.test(n) && !/unins/i.test(n); }) || '';
    } catch (e) { /* none */ }
  }
  const miss = log && (String(log[1]).match(new RegExp(`(?:cannot open|can't open|couldn't open|failed to open|not found|no such file|missing)[^\\n]*?([^\\s'"\\\\/:]+\\.${EXT})\\b`, 'i')) || String(log[1]).match(new RegExp(`([^\\s'"\\\\/:]+\\.${EXT})\\b[^\\n]*?(?:not found|missing|cannot be opened)`, 'i')));
  if (miss && gotFlag) { rootFlag = gotFlag[1]; need = [miss[1]]; }
  // It started without the player's game files (they aren't where it reads them): the launcher's
  // own setup puts them there, when it knows this game.
  const noFiles = !rootFlag && !setupExe && /entrypoint xex not found|game files (?:are )?not (?:found|installed)|game data was not found|no game in .{0,160}asking for it|no executable found \(probed|game files not found at|cannot load [^\n]{0,200}default\.xbe|the \.dat file is missing|the game is not set up yet/i.test(said);
  const copySetup = noFiles && !!(S.easy && S.easy.has(p.id));
  // A laptop with two graphics cards, the game not set to the faster one yet.
  let gpuc = null;
  if ((gpu || vkLoader) && S.os === 'windows') { try { gpuc = await invoke('gpu_choice', { id: p.id }); } catch (e) { /* none */ } }
  // Only when nothing the message says first applies, so the message and the main button agree.
  const twoGpu = !!(gpuc && gpuc.cards >= 2 && !gpuc.fast) && !(modName || sac || badOpts || rootFlag || noFiles || eboot || needTU || dotnet || noWrite || blocked || noRom || gpuReset || (lookOn && p.secs < 30) || inPage || gl || vram || armProton || umuRt || glibc || protonGap || noLib || oom || python || noCmd);
  // A fix it can offer: that's the main button, and sending the report comes second.
  const fix = armProton || sac || otherBuild || badOpts || rootFlag || dotnet || noWrite || lookOn || glSetting || (dllVC && !oldVulkan) || dllDX || python || setupExe || usage || copySetup || twoGpu || oldCopies || (madeOnly && S.easy && S.easy.has(p.id));
  // A plain exit code after a real run (30 seconds or more), on Windows (on Linux every code looks
  // plain, crashes too), with nothing found wrong: it may be how this game always closes.
  const plainExit = S.os === 'windows' && p.secs >= 30 && Math.abs(Number(p.code)) < 256 && !fix;
  // A crash inside the game itself (memory errors, an unhandled exception), with nothing the launcher
  // can change: its developer is who can fix it, so reporting to them comes first.
  const gameOwn = !fix && !modName && !!(a && a.repo) && ([-1073741819, 3221225477, -1073740940, 3221226356, -1073740791, 3221226505, -1073741571, 3221225725].includes(Number(p.code)) || /unhandled exception|access violation|segmentation fault|stack overflow|panicked at|fatal error|assertion failed/i.test(said));
  // What players found for this crash, and whether the update before it is the likely cause.
  const fixSaid = knownFix(a, said + '\n' + String(p.said || ''), g.tag);
  // The line a recomp's runtime writes when it stops itself ("FATAL: …"): the cause, in its words.
  const fatalLine = ((String((log && log[1]) || '').match(/^.*\bFATAL:.*$/gm) || []).pop() || '').trim().slice(0, 300);
  const bad = badVersion(a, g.tag);
  const goBack = !fix && !modName && (bad ? (bad.use || g.prev_tag) : updateBroke(g) ? g.prev_tag : '');
  const m = modal(`<h2>${esc(tf('{name} closed unexpectedly', { name: g.name }))}</h2>
    ${fixSaid ? `<div class="known-fix" role="note"><b>${esc(t('Players Fixed This'))}</b><span dir="auto">${esc(fixSaid)}</span></div>` : ''}
    ${p.said && p.said !== fixSaid ? `<div class="known-fix" role="note"><b>${esc(t('What This Code Means'))}</b><span dir="auto">${esc(p.said)}</span></div>` : ''}
    ${fatalLine ? `<p class="fatal-line"><b>${esc(t('The game\'s last error'))}</b><code>${esc(fatalLine)}</code></p>` : ''}
    ${goBack ? `<p class="go-back-note">${esc(bad ? tf('{version} is crashing for a lot of players. {old} works for most.', { version: g.tag, old: goBack }) : tf('It worked before its last update. {old}, the version you had, is one press away.', { old: goBack }))}</p>` : ''}
    <p>${modName ? esc(tf('It crashed inside a mod ({mod}), not the game itself. Turn that mod off or update it, then play again.', { mod: modName })) : sac ? esc(t('Windows blocked it. Smart App Control (or an app policy on this PC) stops programs that aren\'t signed yet, and this recomp isn\'t. You can turn Smart App Control off in Windows Security, under App & browser control. Windows may not let you turn it back on later, so it\'s your call; the other choice is to wait for the developer to sign their releases.')) : badOpts ? esc(tf('It didn\'t understand its Launch options ({options}). Clear them and play again.', { options: String(g.args).trim() })) : rootFlag && gone ? esc(tf('It\'s looking for your game files in {folder}, which isn\'t there anymore. Pick the folder with your copy of the game and it starts from there next time.', { folder: gone[1].trim() })) : rootFlag && miss ? esc(tf('It couldn\'t find {file} in the folder it was given. Pick the folder with your copy of the game and the launcher finds it inside.', { file: miss[1] })) : rootFlag ? esc(t('It needs to know where your game files are. Pick the folder with your copy of the game and it starts from there next time.')) : noFiles ? esc(copySetup ? t('It started without your game files. Set up your copy of the game once, and it finds them from then on.') : t('It started without your game files: they aren\'t where it reads them. Its developer\'s instructions say where they go.')) : eboot ? esc(t('It needs the decrypted EBOOT.elf from the game\'s update, in the update\'s USRDIR folder. A decrypted one starts with the letters ELF; the normal EBOOT.BIN renamed won\'t work. It\'s made from your own update, for example with RPCS3.')) : needTU ? esc(t('It needs the game\'s title update (the last patch for the game) along with your disc. Its developer\'s instructions say which one and where it goes.')) : dotnet ? esc(t('It needs Microsoft .NET, and the version it needs isn\'t on this PC. Install it, then play again.')) : noWrite ? esc(t('It can\'t write to its own folder, which is somewhere Windows protects (like Program Files). Move it to your Recomp Library in its Manage tab, then play again.')) : blocked ? esc(t('It couldn\'t make a folder inside its own folder. Windows Security may be blocking it: in Virus & threat protection, Ransomware protection, turn off Controlled folder access or allow this game through it, then play again.')) : noRom ? esc(t('It asks for your ROM in its own window the first time it starts, and none was picked. Play again and choose your ROM when it asks.')) : gpuReset ? esc(t('Your graphics driver stopped responding while the game was drawing. Update your graphics driver from NVIDIA, AMD or Intel; if it keeps happening, try another graphics option in its Game settings.')) : lookOn && p.secs < 30 ? esc(t('It closed soon after starting with a shader on. Some games don\'t get along with ReShade: try it without the shader.')) : inPage ? esc(t('Windows couldn\'t read part of the game from its drive while it ran. That happens when a network folder or a USB drive drops out for a moment. Move it to a drive inside this PC (Move to another drive, in its menu), then play again.')) : gl ? esc(glSetting ? tf('Your graphics driver couldn\'t start OpenGL for it. Switch {setting} in its Game settings, or update your graphics driver, then play again.', { setting: glSetting.label }) : t('Your graphics driver couldn\'t start OpenGL for it. Update your graphics driver from NVIDIA, AMD or Intel, then play again.')) : vram ? esc(t('Your graphics card ran out of memory for it. Close other games and apps using it, lower its resolution in Game settings, then play again.')) : armProton ? esc(pfxGone ? t('The Proton it ran through is missing the files it sets up a game with, so it can\'t start anything. Press Install GE-Proton to get a complete one, then play again.') : t('The Proton it ran through is built for a different kind of processor, so it can\'t run here. Press Install GE-Proton in Settings → General to get the right one, then play again.')) : umuRt ? esc(t('umu-launcher\'s Steam Linux Runtime didn\'t finish downloading, so it can\'t start games. Delete the steamrt folders in ~/.local/share/umu and play again, or update umu-launcher.')) : glibc ? esc(t('This Linux build was made for a newer Linux than yours (it needs a newer glibc). Its Windows version usually runs fine through Proton: switch to it on the game\'s page, or update your system.')) : protonGap ? esc(t('The Proton it ran through is missing a part of Windows this game needs. Press Install GE-Proton in Settings → General and play again: the launcher uses the newest Proton it finds.')) : noLib ? esc(tf('It needs {file}, which isn\'t on this PC. Install it with your system\'s package manager, then play again.', { file: noLib[1] })) : oom ? esc(t('Your PC ran out of memory for it. Close other games and apps, check that Windows\' page file is on (it\'s under Advanced system settings → Performance → Virtual memory), then play again.')) : python ? esc(t('It needs Python 3, which isn\'t on this PC. Install it from python.org (tick Add python.exe to PATH in its installer), then play again.')) : noCmd ? esc(t('It runs a command that isn\'t installed on this PC. Its developer\'s instructions say what to install first.')) : vkLoader && !twoGpu ? esc(t('Vulkan couldn\'t use your graphics card. Update your graphics driver from NVIDIA, AMD or Intel, and close overlays and recorders (like RivaTuner, OBS or Overwolf), then play again.')) : (gpu || vkLoader) && twoGpu ? esc(t('This PC has two graphics cards, and it may have started on the slower one. Use the Faster Graphics Card and Windows starts it on the faster one; if it still won\'t start, update your graphics drivers.')) : gpu ? esc(t('Your graphics card or its driver can\'t run it: it needs DirectX 12 or Vulkan. Update your graphics driver from NVIDIA, AMD or Intel, then play again.')) : oldCopies ? esc(tf('It came with older copies of {files} than Windows has, and uses those instead. Move them aside and it uses Windows\' newer ones.', { files: oldCopies.join(', ') })) : oldVulkan ? esc(tf('Your graphics driver is out of date (its Vulkan is {version}; recomps need 1.3 or newer). Update it from NVIDIA, AMD or Intel, then play again.', { version: dllf.vulkan_old })) : madeOnly ? esc(tf('It needs {files}, which it makes itself from your copy of the game. Set up your game copy (or run its setup), then play again.', { files: made.join(', ') })) : dllVC ? esc(t('Windows couldn\'t load a file it needs: a DLL that\'s missing or too old. Install the latest Microsoft Visual C++ Redistributable (x64), keep Windows up to date, then play again.')) : dllDX ? esc(tf('It needs the older DirectX runtime (June 2010), which Windows doesn\'t come with. Missing: {files}', { files: lacking.join(', ') })) : dllOwn ? esc(tf('It needs files that aren\'t on this PC: {files}. They should come with the game. Reinstall it; if they\'re still missing, its developer needs to know.', { files: lacking.join(', ') })) : setupExe ? esc(t('It needs its setup run first, to unpack the game from your copy.')) : toolRan ? esc(t('It printed how it\'s used and stopped. That\'s usually one of the game\'s tools, not the game. Choose the program that starts the game.')) : usage ? esc(t('It needs to be started with your game files. Its developer\'s instructions say what to add; put that in its Launch options.')) : wrongDisc ? esc(t('The disc image it was given isn\'t this game\'s, or it\'s missing the game\'s files. Pick your own disc image of the game this recomp is made for.')) : otherBuild ? esc(t('This recomp comes with a program for each version of the game, and the one it started isn\'t for your copy. Choose the program that matches your copy and play again.')) : wrongCopy ? esc(t('Your copy of the game isn\'t the version this recomp is made for. Its developer\'s instructions say which one to use (region and revision).')) : p.said ? esc(p.said) : miss ? esc(tf('It couldn\'t find {file}, part of your copy of the game. Put your game files where its developer\'s instructions say, then play again.', { file: miss[1] })) : esc(p.secs < 5 ? tf('It stopped with code {code} right after it started. That usually means it couldn\'t find your game files or something it needs on this PC.', { code: codeShown }) : tf('It stopped with code {code}.', { code: codeShown }))}</p>
    ${log ? `<div class="sec-h" style="margin:4px 0 6px"><span class="muted" style="font-size:13px">${esc(tf('From {file}', { file: log[0] }))}</span><button type="button" class="linkbtn" data-x="copy">${esc(t('Copy details'))}</button></div><pre class="logbox">${esc(log[1])}</pre>` : ''}
    <textarea class="field" id="c-note" rows="2" placeholder="${esc(t('What were you doing when it closed? A sentence helps the developer find it.'))}" aria-label="${esc(t('What happened'))}"></textarea>
    <div class="acts grid2">${evenActs([`<button type="button" class="btn" data-x="folder">${esc(t('Open its folder'))}</button>`, `<button type="button" class="btn${!fix && p.secs < 5 && !goBack && !gameOwn ? ' main' : ''}" data-x="trouble">${esc(t('Troubleshoot'))}</button>`, `<button type="button" class="btn${fix || p.secs < 5 || gameOwn || goBack ? '' : ' main'}" data-x="send">${esc(t('Send it to Ripalda'))}</button>`, `<button type="button" class="btn" data-x="ghreport">${esc(t('Report on GitHub'))}</button>`,
      modName ? `<button type="button" class="btn" data-x="copy">${esc(t('Copy details'))}</button>` : armProton ? `<button type="button" class="btn main" data-x="geproton">${esc(t('Install GE-Proton'))}</button>` : sac ? `<button type="button" class="btn main" data-x="sac">${esc(t('Open Windows Security'))}</button>` : otherBuild ? `<button type="button" class="btn main" data-x="pickexe">${esc(t('Choose Program'))}</button>` : badOpts ? `<button type="button" class="btn main" data-x="clearopts">${esc(t('Clear launch options'))}</button>` : rootFlag ? `<button type="button" class="btn main" data-x="gdr">${esc(t('Pick your game files'))}</button>` : copySetup ? `<button type="button" class="btn main" data-x="copysetup">${esc(t('Set Up Your Game Copy'))}</button>` : twoGpu ? `<button type="button" class="btn main" data-x="fastgpu">${esc(t('Use the Faster Graphics Card'))}</button>` : dotnet ? `<button type="button" class="btn main" data-x="dotnet">${esc(t('Get .NET'))}</button>` : noWrite ? `<button type="button" class="btn main" data-x="manage">${esc(t('Manage'))}</button>` : lookOn ? `<button type="button" class="btn main" data-x="nolook">${esc(t('Play Without the Shader'))}</button>` : glSetting ? `<button type="button" class="btn main" data-x="gset">${esc(t('Game settings'))}</button>` : oldCopies ? `<button type="button" class="btn main" data-x="dllaside">${esc(t('Move Them Aside'))}</button>` : madeOnly && S.easy && S.easy.has(p.id) ? `<button type="button" class="btn main" data-x="copysetup">${esc(t('Set Up Your Game Copy'))}</button>` : dllVC && !oldVulkan ? `<button type="button" class="btn main" data-x="vcredist">${esc(t('Get Visual C++'))}</button>` : dllDX ? `<button type="button" class="btn main" data-x="dxruntime">${esc(t('Get DirectX'))}</button>` : python ? `<button type="button" class="btn main" data-x="python">${esc(t('Get Python'))}</button>` : setupExe ? `<button type="button" class="btn main" data-x="setup">${esc(t('Run its setup'))}</button>` : toolRan ? `<button type="button" class="btn main" data-x="pickexe">${esc(t('Choose Program'))}</button>` : usage ? `<button type="button" class="btn main" data-x="opts">${esc(t('Launch options'))}</button>` : goBack ? `<button type="button" class="btn main" data-x="goback">${esc(tf('Go Back to {version}', { version: goBack }))}</button>` : a ? `<button type="button" class="btn${gameOwn ? ' main' : ''}" data-x="report">${esc(t('Report to the Developer'))}</button>` : `<button type="button" class="btn" data-x="copy">${esc(t('Copy details'))}</button>`, plainExit ? `<button type="button" class="btn" data-x="mine">${esc(t('I Closed It Myself'))}</button>` : '', `<button type="button" class="btn" data-x="no">${esc(t('Close'))}</button>`])}</div>`, async (x, close) => {
    if (x.dataset.x === 'copysetup') { close(); delete S.setup[p.id]; setupModal(p.id); }
    if (x.dataset.x === 'dllaside') { try { const moved = await invoke('dll_aside', { id: p.id }); close(); toast(moved.length ? tf('Moved aside: {files}. Press Play to try it.', { files: moved.join(', ') }) : t('Nothing needed moving.')); } catch (e) { fail(e); } }
    if (x.dataset.x === 'fastgpu') { try { await invoke('prefer_fast_gpu', { id: p.id }); close(); toast(t('It starts on the faster graphics card from now on. Press Play to try it.')); } catch (e) { fail(e); } }
    if (x.dataset.x === 'mine') {
      const q = Object.assign({}, (S.settings.ui || {}).quiet_exits || {});
      q[p.id] = [...new Set([...(q[p.id] || []), Number(p.code)])];
      await saveUi({ quiet_exits: q }, true);
      close();
      toast(t('Got it. When it closes this way again, there\'s no crash window.'));
    }
    if (x.dataset.x === 'copy') { try { await navigator.clipboard.writeText(details); toast(t('Copied.')); } catch (e) { toast(t('Couldn\'t copy.'), true); } }
    if (x.dataset.x === 'folder') invoke('open_folder', { id: p.id }).catch(fail);
    if (x.dataset.x === 'trouble') { close(); return troubleshootModal(p.id, codeShown); }
    if (x.dataset.x === 'sac') invoke('open_app_control').catch(fail);
    if (x.dataset.x === 'pickexe') { close(); const el = document.createElement('i'); el.dataset.id = p.id; el.dataset.act = 'setexe'; return act(el, new Event('click')); }
    if (x.dataset.x === 'setup') { try { await invoke('run_program', { id: p.id, rel: setupExe }); close(); toast(t('Its setup is open. Play again when it\'s done.')); } catch (e) { fail(e); } }
    if (x.dataset.x === 'opts') { close(); launchOptions(p.id); }
    if (x.dataset.x === 'gset') { close(); S.nextTab = 'settings'; go('game', p.id); }
    if (x.dataset.x === 'dotnet') invoke('open_url', { url: dotnetUrl }).catch(fail);
    if (x.dataset.x === 'python') invoke('open_url', { url: S.os === 'linux' ? 'https://www.python.org/downloads/source/' : 'https://www.python.org/downloads/windows/' }).catch(fail);
    if (x.dataset.x === 'manage') { close(); S.nextTab = 'installed'; go('game', p.id); }
    if (x.dataset.x === 'nolook') { close(); if (!S.looks[p.id] || S.looks[p.id] === 'loading') S.looks[p.id] = { look: g.look, amount: g.look_amount }; await setLook(p.id, ''); playGame(p.id); }
    if (x.dataset.x === 'vcredist') { close(); installVcredist(); }
    if (x.dataset.x === 'dxruntime') invoke('open_url', { url: 'https://www.microsoft.com/en-us/download/details.aspx?id=35' }).catch(fail);
    if (x.dataset.x === 'geproton') { close(); geInstall(); }
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
    if (x.dataset.x === 'goback' && goBack) { close(); startInstall(p.id, undefined, goBack).catch(fail); return; }
    if (x.dataset.x === 'ghreport') {
      const note = String(($('#c-note', m) || {}).value || '').trim();
      // The graphics card, if the PC answers quickly; the report goes without it otherwise.
      const pc = await Promise.race([invoke('pc_specs').catch(() => null), new Promise((r) => setTimeout(() => r(null), 3000))]);
      const url = ghCrashUrl({ reason: `${g.name} closed with code ${codeShown}`, note, version: S.version,
        os: `${OS_NAME[S.os] || S.os}${pc && pc.osver ? ` ${pc.osver}` : ''}${S.arch ? ` (${S.arch})` : ''}`, gpu: pc && pc.gpu,
        game: `${g.name} ${g.tag || ''} (${p.id})`.replace(/\s+/g, ' '), exit: `Closed with code ${codeShown} after ${p.secs} s`,
        file: log && log[0], text: log && log[1] });
      invoke('open_url', { url }).then(() => toast(t('The report is filled in. Attach the full log, then submit it on the page.'))).catch(fail);
    }
    if (x.dataset.x === 'report' && a) {
      // A new issue on the game's own page, filled in: what happened, the versions and the end of its log.
      const note = String(($('#c-note', m) || {}).value || '').trim();
      const tail = log ? String(log[1]).split('\n').slice(-40).join('\n').slice(-1800) : '';
      const title = `Crash: closed with code ${codeShown} after ${p.secs} s`;
      const body = [note ? `**What happened:** ${note}` : '**What happened:** ', '', `- Game: ${g.name} ${g.tag || ''}`, `- System: ${OS_NAME[S.os]} (${S.arch || ''})`, `- Started from RecompLauncher ${S.version}`, `- Closed with code ${codeShown} after ${p.secs} s`, ...(tail ? ['', `From ${log[0]}:`, '```', tail, '```'] : [])].join('\n');
      // SourceForge, or a report link from the catalog: its page, with the details on the clipboard.
      if (isSF(a.repo) || /^https:\/\//i.test(String(a.report_url || ''))) {
        try { await navigator.clipboard.writeText(`${title}\n\n${body}`); } catch (e) { /* the window has the details anyway */ }
        invoke('open_url', { url: reportUrl(a) }).then(() => toast(t('The details are copied. Paste them on the page that opened.'))).catch(fail);
        return;
      }
      const q = isLab(a.repo) ? `?issue[title]=${encodeURIComponent(title)}&issue[description]=${encodeURIComponent(body)}` : `/new?title=${encodeURIComponent(title)}&body=${encodeURIComponent(body)}`;
      const url = (isLab(a.repo) ? repoIssues(a.repo) : repoIssues(a.repo)) + q;
      try { await navigator.clipboard.writeText(details); } catch (e) { /* the page has the details anyway */ }
      invoke('open_url', { url: url.length > 7500 ? repoIssues(a.repo) + (isLab(a.repo) ? '' : '/new') : url }).then(() => toast(t('The details are filled in, and copied too. Add anything you remember, then submit it on the page.'))).catch(fail);
    }
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
  const m = modal(`<h2>${esc(t('Add From GitHub or GitLab'))}</h2><p>${esc(t('Paste the link to a recomp\'s page on GitHub, GitLab or SourceForge. The launcher reads its releases and adds it to your Library, ready to install and keep up to date.'))}</p>
    <input class="field" id="ghl" type="url" placeholder="https://github.com/owner/name" spellcheck="false" autocomplete="off" aria-label="${esc(t('GitHub or GitLab link'))}">
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
      err.textContent = t(String(e && e.message ? e.message : e)); err.hidden = false;
      x.disabled = false; x.textContent = was;
    }
  }, t('Add From GitHub or GitLab'));
  const f = $('#ghl', m); f.focus();
  f.addEventListener('keydown', (e) => { if (e.key === 'Enter') $('[data-x="yes"]', m).click(); });
}

// Consoles to pick from: the catalog's, then the usual ones.
const MORE_CONSOLES = [['n64', 'Nintendo 64'], ['x360', 'Xbox 360'], ['xbox', 'Xbox'], ['ps1', 'PlayStation'], ['ps2', 'PlayStation 2'], ['ps3', 'PlayStation 3'], ['psp', 'PSP'], ['gc', 'GameCube'], ['wii', 'Wii'], ['wiiu', 'Wii U'], ['switch2', 'Nintendo Switch 2'], ['switch', 'Nintendo Switch'], ['3ds', 'Nintendo 3DS'], ['nds', 'Nintendo DS'], ['gba', 'Game Boy Advance'], ['gb', 'Game Boy'], ['snes', 'Super Nintendo'], ['nes', 'NES'], ['genesis', 'Sega Genesis'], ['saturn', 'Sega Saturn'], ['dc', 'Dreamcast'], ['pc', 'PC']];
function consoleList() {
  const seen = new Set(), out = [];
  for (const o of (S.cat && S.cat.origins) || []) if (!seen.has(o.name.toLowerCase())) { seen.add(o.name.toLowerCase()); seen.add(o.id); out.push([o.id, o.name]); }
  for (const [id, n] of MORE_CONSOLES) if (!seen.has(id) && !seen.has(n.toLowerCase())) { seen.add(id); out.push([id, n]); }
  return out.sort((a, b) => a[1].localeCompare(b[1]));
}
function consoleOptions(cur) {
  return `<option value="">${esc(t('Not sure'))}</option>` + consoleList().map(([id, n]) => `<option value="${esc(id)}"${id === cur ? ' selected' : ''}>${esc(n)}</option>`).join('');
}

// A list under a text field as you type, to pick from instead of guessing the exact name: Up and
// Down then Enter, or a click. `find(q, later)` gives the first list and can send a longer one later.
function suggestBox(input, find, pick) {
  const box = document.createElement('div');
  box.className = 'sugg'; box.hidden = true; box.setAttribute('role', 'listbox');
  input.setAttribute('autocomplete', 'off'); input.setAttribute('aria-autocomplete', 'list');
  input.parentElement.classList.add('sugg-host');
  input.after(box);
  let items = [], at = -1, seq = 0, tmr;
  const draw = () => {
    box.hidden = !items.length;
    box.innerHTML = items.map((x, i) => `<button type="button" role="option" class="sg-it${i === at ? ' on' : ''}" data-i="${i}" aria-selected="${i === at}" tabindex="-1"><b>${esc(x.label)}</b>${x.sub ? `<span>${esc(x.sub)}</span>` : ''}${x.kind ? `<small>${esc(x.kind)}</small>` : ''}</button>`).join('');
  };
  const run = async () => {
    const q = input.value.trim(), my = ++seq;
    if (q.length < 2) { items = []; return draw(); }
    // The longer list can come before the first one is shown (an answer kept from before).
    let late = null, shown = false;
    const got = await find(q, (more) => { if (my !== seq) return; late = more; if (shown && document.activeElement === input) { items = more; draw(); } });
    if (my !== seq || document.activeElement !== input) return;
    shown = true;
    items = late || got || []; at = -1; draw();
  };
  input.addEventListener('input', () => { clearTimeout(tmr); tmr = setTimeout(run, 160); });
  input.addEventListener('keydown', (e) => {
    if (box.hidden || !items.length) return;
    if (e.key === 'ArrowDown') { e.preventDefault(); at = (at + 1) % items.length; draw(); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); at = at <= 0 ? items.length - 1 : at - 1; draw(); }
    else if (e.key === 'Enter' && at >= 0) { e.preventDefault(); e.stopPropagation(); choose(at); }
    else if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); items = []; draw(); }
  });
  box.addEventListener('mousedown', (e) => e.preventDefault());
  box.addEventListener('click', (e) => { e.stopPropagation(); const b = e.target.closest('[data-i]'); if (b) choose(+b.dataset.i); });
  input.addEventListener('blur', () => setTimeout(() => { if (document.activeElement !== input) { clearTimeout(tmr); seq++; items = []; draw(); } }, 150));
  function choose(i) { const x = items[i]; clearTimeout(tmr); items = []; draw(); seq++; if (x) pick(x); }
}

// Games to pick by a bit of their name: the ones in Discover first (with their console), consoles
// that fit, then SteamGridDB's (with the player's own key).
function gameFinder(key, { consoles = true, sgdbOnly = false } = {}) {
  return async (q, later) => {
    const ql = q.toLowerCase(), words = ql.split(/\s+/).filter(Boolean);
    const fits = (n) => { const l = n.toLowerCase(), ws = l.split(/[^\p{L}\p{N}]+/u).filter(Boolean); return l.includes(ql) || words.every((w) => ws.some((x) => x.startsWith(w))); };
    const seen = new Set(), local = [];
    if (!sgdbOnly) {
      for (const a of (S.cat && S.cat.apps) || []) {
        const n = String(a.game || '').trim();
        if (!n || seen.has(n.toLowerCase()) || !fits(n)) continue;
        seen.add(n.toLowerCase());
        local.push({ label: n, sub: a.origin ? originName(a.origin) : '', game: n, origin: a.origin || '' });
      }
      local.sort((x, y) => (y.label.toLowerCase().startsWith(ql) - x.label.toLowerCase().startsWith(ql)) || x.label.localeCompare(y.label));
    }
    const cons = consoles && !sgdbOnly ? consoleList().filter(([, n]) => fits(n)).slice(0, 2).map(([id, n]) => ({ kind: t('Console'), label: n, origin: id })) : [];
    const first = [...local.slice(0, 6), ...cons];
    if (key) {
      invoke('art_search', { term: q, key }).then((r) => {
        const more = (r || []).filter((x) => x && x.name && !seen.has(String(x.name).toLowerCase())).slice(0, 6)
          .map((x) => ({ kind: 'SteamGridDB', label: String(x.name), sub: x.year ? String(x.year) : '', game: String(x.name), sgdb: x.id }));
        if (more.length) later([...first, ...more]);
      }).catch(() => {});
    }
    return first;
  };
}

// The game's details to check before it's added (`link`) or to change after (`id`): name, which
// game it is (picked from a list as you type), its console and its art (SteamGridDB's, or the
// player's own pictures).
function gameInfoModal({ link, info, id }) {
  const key = (S.settings.ui || {}).sgdb_key || '';
  const cur = { name: info.name || '', game: info.game || '', origin: info.origin || '', cover: info.cover || '', hero: info.hero || '', logo: info.logo || '' };
  // Pictures from this PC picked here, by kind: { src, data }. They're kept with the game.
  const own = {};
  const art = { covers: [], heroes: [], logos: [] };
  const take = (a, fresh) => {
    art.covers = (a && a.covers) || []; art.heroes = (a && a.heroes) || []; art.logos = (a && a.logos) || [];
    for (const [k, list] of [['cover', art.covers], ['hero', art.heroes], ['logo', art.logos]]) {
      if (own[k] && cur[k] === own[k].data) continue;
      if (list.length && (!cur[k] || link || fresh)) cur[k] = artUrl(list[0]);
    }
  };
  take(info.art);
  if (link && !cur.hero) cur.hero = info.hero_default || '';
  const sim = info.similar ? `<div class="gi-sim"><span>${esc(tf('This looks like {name}, which is already in Discover.', { name: info.similar.name }))}</span><button type="button" class="btn sm" data-x="similar">${esc(t('See It'))}</button></div>` : '';
  const hint = key ? t('Type a bit of the game\'s name and pick it from the list. Click the cover or background to see all the art, or use your own pictures.')
    : t('Type a bit of the game\'s name and pick it from the list. Click the cover or background to use your own pictures. With your own SteamGridDB key (Settings → Games and folders), art comes by itself.');
  const m = modal(`<h2>${esc(link ? t('Check the Details') : t('Edit Game Info'))}</h2>${sim}
    <div class="gi"><div class="gi-cover" id="gi-cover" data-x="choose" title="${esc(t('Choose Art…'))}"></div>
      <div class="gi-fields">
        <label><span>${esc(t('Name'))}</span><input class="field" id="gi-name" value="${esc(cur.name)}" maxlength="120" spellcheck="false"></label>
        <label><span>${esc(t('Game'))}</span><input class="field" id="gi-game" value="${esc(cur.game)}" maxlength="120" spellcheck="false" placeholder="${esc(t('Type a bit of the game\'s name'))}"></label>
        <label><span>${esc(t('Console'))}</span><select class="field sel" id="gi-origin" aria-label="${esc(t('Console'))}">${consoleOptions(cur.origin)}</select></label>
      </div></div>
    <div class="gi-hero" id="gi-hero" aria-label="${esc(t('Background'))}" data-x="choose" title="${esc(t('Choose Art…'))}"><div class="gi-heroimg" id="gi-heroimg"></div><div class="gi-logo" id="gi-logo"></div></div>
    <p class="muted gi-hint" id="gi-status">${esc(hint)}</p>
    <p class="err" id="gi-err" hidden>${esc(info.art_error || '')}</p>
    <div class="acts grid3"><button type="button" class="btn" data-x="no">${esc(t('Cancel'))}</button><button type="button" class="btn" data-x="choose">${esc(t('Choose Art…'))}</button><button type="button" class="btn main" data-x="yes">${esc(link ? t('Add to Library') : t('Save'))}</button></div>`, async (x, close) => {
    const err = $('#gi-err', m);
    const read = () => { cur.name = $('#gi-name', m).value.trim(); cur.game = $('#gi-game', m).value.trim(); cur.origin = $('#gi-origin', m).value; };
    switch (x.dataset.x) {
      case 'similar': close(); return go('game', info.similar.id);
      case 'choose': return artPicker(art, cur, own, { id: id || null, key, game: () => ($('#gi-game', m).value.trim() || $('#gi-name', m).value.trim()) }, () => paint());
      case 'yes': {
        read();
        if (!cur.name) return $('#gi-name', m).focus();
        x.disabled = true;
        // The player's own pictures aren't web addresses: they're kept for the game once it's in.
        const mine = Object.keys(own).filter((k) => own[k] && cur[k] === own[k].data);
        const sent = Object.assign({}, cur);
        for (const k of mine) sent[k] = '';
        try {
          let gid = id;
          if (link) {
            const r = await invoke('add_github', { link, info: sent });
            gid = r.id;
            for (const k of gid ? mine : []) await window.ownArt.save(gid, k, own[k]);
            close();
            await loadCatalog(false);
            if (!r.known && !S.library[r.id]) await saveUi({ saved: Object.assign({}, saved(), { [r.id]: Math.floor(Date.now() / 1000) }) }, true);
            toast(tf('{name} is in your Library. Install it whenever you like.', { name: r.name }));
            go('game', r.id);
          } else {
            // Own pictures don't touch the web art kept for the game (it's there again if they're taken off).
            for (const k of ['cover', 'hero', 'logo']) if (mine.includes(k) || !sent[k]) delete sent[k];
            await invoke('update_mine', { id, info: sent });
            for (const k of mine) await window.ownArt.save(gid, k, own[k]);
            // Web art picked here for a kind that had the player's own picture: that picture steps aside.
            const had = S.library[id] || (S.arts || {})[id] || {};
            for (const k of ['cover', 'hero', 'logo']) if (!mine.includes(k) && sent[k] && sent[k] !== info[k] && had[k + '_file']) await window.ownArt.clear(id, k);
            close();
            await loadCatalog(false);
            render(true);
            toast(t('Saved.'));
          }
        } catch (e) { err.textContent = String(e && e.message ? e.message : e); err.hidden = false; x.disabled = false; }
      }
    }
  }, link ? t('Check the Details') : t('Edit Game Info'));
  // What shows: the player's own picture for a kind until other art is picked here.
  const shown = (k) => { const f = id && (S.library[id] || (S.arts || {})[id] || {})[k + '_file']; return f && cur[k] === (info[k] || '') ? 'local:' + f : cur[k]; };
  function paint() {
    const box = $('#gi-cover', m);
    box.innerHTML = shown('cover') ? pic(shown('cover')) : `<span>${esc(cur.name.slice(0, 2).toUpperCase())}</span>`;
    $('#gi-heroimg', m).innerHTML = shown('hero') ? pic(shown('hero')) : '';
    $('#gi-logo', m).innerHTML = shown('logo') ? pic(shown('logo')) : '';
    $('#gi-hero', m).classList.toggle('empty', !shown('hero'));
  }
  // Art follows the Game name: a new name finds its art on its own (with a SteamGridDB key).
  let looked = (cur.game || cur.name).trim().toLowerCase();
  const status = (txt) => { $('#gi-status', m).textContent = txt; };
  let artSeq = 0;
  const artBy = async (g, sgdb) => {
    if (!key) return;
    const err = $('#gi-err', m), my = ++artSeq;
    err.hidden = true; status(t('Finding art…'));
    try {
      const a = sgdb ? await invoke('art_for', { sgdb, key }) : await invoke('github_art', { game: g, sgdbKey: key });
      if (my !== artSeq) return; // a newer pick is on its way
      take(a, true);
      paint();
      if (!art.covers.length) { err.textContent = t('SteamGridDB has no art under that name. Try the game\'s full name.'); err.hidden = false; }
    } catch (e) { if (my === artSeq) { err.textContent = String(e && e.message ? e.message : e); err.hidden = false; } }
    if (my === artSeq) status(hint);
  };
  const refind = () => {
    const g = ($('#gi-game', m).value.trim() || $('#gi-name', m).value.trim());
    if (!g || g.toLowerCase() === looked) return;
    looked = g.toLowerCase();
    artBy(g);
  };
  $('#gi-game', m).addEventListener('change', refind);
  suggestBox($('#gi-game', m), gameFinder(key), (x) => {
    if (x.origin) { $('#gi-origin', m).value = x.origin; if (window.syncDrops) window.syncDrops(); }
    // A console picked: the bit typed to find it isn't the game's name.
    if (!x.game) { $('#gi-game', m).value = ''; looked = $('#gi-name', m).value.trim().toLowerCase(); return; }
    $('#gi-game', m).value = x.game;
    looked = x.game.toLowerCase();
    artBy(x.game, x.sgdb);
  });
  paint();
  const e = $('#gi-err', m); e.hidden = !info.art_error;
  $('#gi-name', m).focus();
}
// Troubleshoot (Beta): what could keep a game from starting, each with a fix where there is one.
const TS_FIX = {
  locate: [N_('Find Its Folder'), (id) => missingModal(id)],
  dllaside: [N_('Move Them Aside'), async (id) => { const moved = await invoke('dll_aside', { id }); toast(moved.length ? tf('Moved aside: {files}. Press Play to try it.', { files: moved.join(', ') }) : t('Nothing needed moving.')); }],
  setup: [N_('Set Up Your Game Copy'), (id) => { delete S.setup[id]; return setupModal(id); }],
  quiet: [N_('Turn It Back On'), async (id) => { const q = Object.assign({}, (S.settings.ui || {}).quiet_exits || {}); delete q[id]; await saveUi({ quiet_exits: q }, true); toast(t('The crash window shows for it again.')); }],
  setexe: [N_('Choose Program…'), (id) => actOn(id, 'setexe')],
  reinstall: [N_('Reinstall'), (id) => actOn(id, 'versions')],
  proton: [N_('Open Settings'), () => { S.setTab = 'pc'; go('settings'); }],
  unblock: [N_('Unblock'), async (id) => { const n = await invoke('unblock_game', { id }); toast(tn(n, 'Unblocked 1 file.', 'Unblocked {n} files.')); }],
  vcredist: [N_('Install It'), () => installVcredist()],
  copy: [N_('Pick Your Copy'), (id) => setupModal(id)],
  move: [N_('Move…'), (id) => moveModal(id)],
  files: [N_('Open Its Folder'), (id) => invoke('open_folder', { id })],
  manual: [N_('Set Up By Hand…'), (id) => manualModal(id)],
  dxruntime: [N_('Get It'), () => invoke('open_url', { url: 'https://www.microsoft.com/en-us/download/details.aspx?id=35' })],
  lookroute: [N_('Try Both Ways'), async (id) => { await invoke('look_reset_route', { id }); toast(t('Done. Press Play to try the Look again.')); }],
  clean: [N_('Try a Clean Start'), async (id) => { await invoke('clean_start_next', { id }); playGame(id); }],
};
// What Troubleshoot can say (from troubleshoot in lib.rs), so they get translated.
const TS_TEXT = [
  N_('The program you picked isn\'t there'),
  N_('An update may have renamed it. Pick it again in Set Up By Hand.'),
  N_('You can set it up by hand'),
  N_('Point the launcher at its program and your game files yourself.'),
  N_('It\'s set up by hand'),
  N_('It starts the way you set it up, not the launcher\'s way.'),
  N_('Your game files aren\'t where you pointed it anymore.'),
  N_('It needs files that aren\'t on this PC'),
  N_('Missing: {files}. It needs the Visual C++ runtime, and part of it is missing.'),
  N_('Missing: {files}. It needs the older DirectX runtime (June 2010), which Windows doesn\'t come with.'),
  N_('Missing: {files}. They should come with the game. Reinstall it; if they\'re still missing, its developer needs to know.'),
  N_('Missing: {files}. Install them with your system\'s package manager (the names are a good search), then press Play again.'),
  N_('Its folder is in OneDrive'),
  N_('Games often can\'t run from a folder OneDrive syncs: some files may only be online, and syncing locks them while the game runs.'),
  N_('The shader only tries one way in'),
  N_('Since a start that crashed, its shader goes in through {way} only. If the shader doesn\'t show, let it try both ways again.'),
  N_('Its folder\'s path has letters some games can\'t read'),
  N_('Many recomps can\'t open files in a folder whose path has accents or letters outside English. Move it to a folder with plain letters, like C:\\Recomp Library.'),
  N_('It says something it needs is missing'),
  N_('It says your game files aren\'t the right ones'),
  N_('How it closed last time'),
  N_('Code {code}'),
  // Reports and their answers (the launcher's words, and the website's).
  N_('Tell the developer what happened and when.'),
  N_('Pick how it plays.'),
  N_('Sign in to see your reports.'),
  N_('Sign in to answer.'),
  N_('Your reports couldn\'t be loaded. Try again later.'),
  N_('Your answer didn\'t go through. Try again later.'),
  N_('Your report is in, but the log couldn\'t be kept.'),
  N_('Logs can be 2 MB at most.'),
  N_('Send the log as a text file, like a .txt or .log.'),
  N_('That\'s a lot at once. Try again later.'),
  N_('That\'s a lot of answers in an hour. Try again a bit later.'),
  N_('That report isn\'t yours, or it\'s gone.'),
  N_('That setting isn\'t a slider.'),
  N_('That isn\'t a number.'),
  N_('That isn\'t a game.'),
  N_('Required files'),
  N_('Shared keys'),
  N_('Choose file'),
  N_('Choose folder'),
  N_('Working…'),
  N_('{ready} of {total} discs selected'),
  N_('{ready} of {total} game files selected'),
  N_('Insert your copies of {game}'),
  N_('Finish choosing this game\'s files in Easy Setup.'),
  N_('Checking the selected files'),
  N_('Files selected.'),
  N_('Your files are set up.'),
  N_('Set this game up from its Easy Setup screen.'),
  N_('That file is already picked for another slot.'),
  N_('That file or folder isn\'t the kind this game asks for here.'),
  N_('Close the game or stop setup first.'),
  N_('The selected files changed. Run setup again.'),
  N_('It came with old copies of files Windows has newer ones of'),
  N_('{files}: the game uses these instead of Windows\' newer ones, and may close with "a DLL it needs is too old". Moving them aside fixes that.'),
  N_('Your graphics driver is out of date'),
  N_('Its Vulkan is version {version}; recomps need 1.3 or newer. Update your graphics driver from NVIDIA, AMD or Intel.'),
  N_('Missing: {files}. The game makes them itself from your copy of the game, with its setup. Set up your game copy (or run its setup), then press Play again.'),
  N_('Your Windows user folder\'s name has letters some games can\'t read'),
  N_('Some recomps keep saves and settings in your user folder ({folder}) and can\'t open it when its name has accents or letters outside English. If it closes right away, that may be why: tell its developer.'),
  N_('That\'s a program, not your copy of the game. Pick the game file itself.'),
  N_('That\'s the game\'s own folder, where the launcher installed it. Pick your copy of the game: the disc image or the folder with the original game\'s files.'),
  N_('That\'s the folder the launcher installs games into. Pick your copy of the game: the disc image or the folder with the original game\'s files.'),
  N_('The Visual C++ runtime is out of date'),
  N_('Newer recomps need a newer one than this PC has. Updating it is quick.'),
  N_('The launcher couldn\'t tell which program starts the game.'),
  N_('Cloud saves are paused for now. Your saves on this PC are safe.'),
  N_('Cloud saves are paused while we move them to new storage. Your saves on this PC are safe.'),
  N_('Cloud saves aren\'t available right now. Your saves on this PC are safe.'),
  N_('This save is too big for cloud saves (over 95 MB). It\'s kept on this PC.'),
  N_('It starts {name}, which looks like one of the game\'s tools'),
  N_('It sits in a folder for tools or a setup\'s leftovers ({folder}). Choose the game\'s own program, where its setup installed it.'),
  N_('Windows didn\'t let the launcher write the game\'s files (access denied). An antivirus may be checking them, or the library folder is one Windows protects. Try again in a minute, or pick another library folder in Settings → Games and Folders.'),
  N_('This is a Windows version, and it needs Proton to play on Linux. Settings → General → GE-Proton installs it in one press, then press Play again.'),
  N_('GE-Proton is only made for x86-64 PCs.'),
  N_('GitHub\'s answer about GE-Proton didn\'t make sense. Try again later.'),
  N_('That isn\'t a GitHub or GitLab link. It looks like github.com/owner/name or gitlab.com/group/name.'),
  N_('That isn\'t a GitHub, GitLab or SourceForge link. It looks like github.com/owner/name, gitlab.com/group/name or sourceforge.net/projects/name.'),
  N_('That file isn\'t in this recomp\'s own releases, so it wasn\'t downloaded.'),
  N_('Windows needs your permission to install it. Press the button again and choose Yes when Windows asks.'),
  N_('That isn\'t an N64 ROM. Pick your game\'s .z64, .n64 or .v64 file.'),
  N_('That\'s a picture, document or video, not your copy of the game. Pick the game file itself.'),
  N_('That setting isn\'t a list of keys.'),
  N_('That isn\'t one of its actions.'),
  N_('The GE-Proton download didn\'t have Proton in it.'),
  N_('Its Linux build needs a newer Linux'),
  N_('It was built for a newer system library (glibc) than this Linux has. Its Windows version usually runs fine through Proton: switch to it from the game\'s page, or update your system.'),
  N_('It needs to be told where your game files are'),
  N_('This recomp reads your extracted game from a folder given to it when it starts, and its page doesn\'t set that up yet. Use Set Up By Hand and pick the folder with your game\'s files (the one with default.xex).'),
  N_('That\'s your game\'s disc image or ROM, not the program that starts it. Pick the recomp\'s own program (usually an .exe in its folder); your copy of the game goes in its setup.'),
  N_('That\'s a compressed file, not a program. Unpack it first, then pick the program inside.'),
  N_('That isn\'t a program. Pick the one that starts the game (usually an .exe, or on Linux the file without an extension or an .AppImage).'),
  N_('That folder is empty. Pick the folder with your copy of the game.'),
  N_('That folder only has pictures, videos or documents in it. Pick the folder with your copy of the game.'),
  N_('Sign in first.'),
  N_('That isn\'t a recomp.'),
  N_('The website didn\'t take it. Try again later.'),
  N_('Wine tools are for Linux.'),
  N_('Windows games need Proton or Wine on this PC first. Settings → General → GE-Proton installs it in one press.'),
  N_('There\'s no Wine prefix yet. Play a Windows game here once and it\'s made.'),
  N_('Pick a Windows program or installer (.exe or .msi).'),
  N_('That isn\'t one of the Wine tools.'),
  N_('Winetricks needs zenity or kdialog to show its window. Install zenity from your distro\'s packages, then try again.'),
  N_('Winetricks isn\'t on this PC. Install winetricks from your distro\'s packages, then try again.'),
  N_('Play a Windows game here once first: that makes the Wine prefix Winetricks works on.'),
  N_('This Proton\'s own Wine couldn\'t be found.'),
  N_('That folder is already one of its save folders.'),
  N_('That folder is inside one of its save folders, or holds one. Pick folders side by side.'),
  N_('It\'s already in your Library.'),
  N_('No updates in 1 month'),
  N_('No updates in {n} months'),
  N_('No updates in 1 year'),
  N_('No updates in {n} years'),
  N_('This build comes as a Flatpak, and Flatpak isn\'t on this PC. Install Flatpak from your system\'s software center, or switch to another build on the game\'s page.'),
  N_('This build comes as a Flatpak, and the RecompLauncher Flatpak is not allowed to install it. Allow it with: flatpak override --user --talk-name=org.freedesktop.Flatpak com.recomplauncher.RecompLauncher. Or switch to another build on the game page.'),
  N_('This Flatpak file doesn\'t say which app it holds, so it can\'t be installed. Switch to another build on the game\'s page.'),
  N_('Installing its Flatpak for the first time. It can take a few minutes; the game starts when it\'s done.'),
  N_('Unpacking the game the first time it\'s played. Later starts are quicker.'),
  N_('That\'s inside the launcher\'s own folder. Pick a folder of yours, on any drive.'),
  N_('The launcher can\'t write to that folder. Pick another one.'),
  N_('The launcher isn\'t allowed to write to the library folder. Pick one in your home folder in Settings → Games and Folders.'),
  N_('The drive couldn\'t read or write the game\'s files (a drive error, not the download). Check the drive or memory card is plugged in and healthy, or pick a library folder on another drive in Settings → Games and Folders.'),
  N_('The drive ran out of space while installing. Free some up, or pick a library folder on another drive in Settings → Games and Folders.'),
  N_('A folder the install was using isn\'t there anymore (a drive was unplugged or renamed, or a network folder dropped). Check the library folder in Settings → Games and Folders and try again.'),
  N_('The GE-Proton download didn\'t match its checksum. Try again.'),
  N_('The Visual C++ runtime is for Windows.'),
  N_('Microsoft\'s download didn\'t answer as expected. Try again later, or get it from microsoft.com.'),
  N_('The installer was cancelled.'),
  N_('The Visual C++ installer didn\'t finish. Run it from microsoft.com to see why.'),
  N_('A site took too long to answer. Try again in a minute.'),
  N_('A shader (ReShade) and your own launch options are on. If it won\'t start, try once without them.'),
  N_('A shader (ReShade) is on. If it won\'t start, try once without it.'),
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
  N_('Your copy of the game may be incomplete'),
  N_('Its size isn\'t one a full dump of the cartridge has. If the game won\'t start, try another copy of your own cartridge.'),
  N_('The shader didn\'t start last time'),
  N_('ReShade didn\'t load in this game, so its shader can\'t show. Play it without the shader.'),
  N_('ReShade didn\'t load in this game. The next time it starts, the shader tries another way in.'),
  N_('Your own launch options are on. If it won\'t start, try once without them.'),
  N_('{error}. Games often can\'t save or set up in protected folders like Program Files.'),
  N_('{gb} GB free. Some games need room for saves and shader caches.'),
  N_('{name} is already open'),
  N_('{path} can\'t be found. Maybe it\'s on a drive that isn\'t connected, or it was moved.'),
];
// Set Up By Hand: the player points at the program and their game files, and says how the game
// gets them. For when the launcher's own setup keeps getting it wrong.
async function manualModal(id) {
  const g = S.library[id];
  if (!g) return;
  let m;
  try { m = await invoke('manual_defaults', { id }); } catch (e) { return fail(e); }
  m = Object.assign({ exe: null, copy: null, how: 'none', into: '', name: '', arg: '' }, m);
  const on = !!g.manual;
  const name = (libApp(g) || {}).name || g.name;
  const show = (p) => (p ? String(p) : '');
  const HOW = [['folder', t('Put Them in Its Folder')], ['arg', t('Give Them to the Program')], ['none', t('It Asks for Them')]];
  const html = () => `<h2>${esc(tf('Set Up {name} By Hand', { name }))}</h2>
    <p class="muted">${esc(t('Point the launcher at the program that starts the game and at your game files, and say how the game gets them. It starts that way from now on, instead of the launcher\'s own setup.'))}</p>
    <div class="mh">
      <div class="mh-row"><div class="mh-k"><b>${esc(t('Program'))}</b><span class="path">${esc(show(m.exe) || t('Not picked'))}</span></div><button type="button" class="btn sm" data-x="exe">${esc(t('Choose…'))}</button></div>
      <div class="mh-row"><div class="mh-k"><b>${esc(t('Your Game Files'))}</b><span class="path">${esc(show(m.copy) || t('None'))}</span></div><span class="mh-b2"><button type="button" class="btn sm" data-x="copy-file">${esc(t('File…'))}</button><button type="button" class="btn sm" data-x="copy-dir">${esc(t('Folder…'))}</button></span></div>
      <div class="mh-how" role="radiogroup" aria-label="${esc(t('How the game gets them'))}">${HOW.map(([k, n]) => `<button type="button" role="radio" aria-checked="${m.how === k}" class="mh-opt${m.how === k ? ' on' : ''}" data-x="how" data-k="${k}">${esc(n)}</button>`).join('')}</div>
      ${m.how === 'folder' ? `<label class="mh-f"><span>${esc(t('Folder inside the game\'s folder (. for the folder itself)'))}</span><input class="field" id="mh-into" value="${esc(m.into || '.')}" spellcheck="false"></label>
        <label class="mh-f"><span>${esc(t('Name it (optional, for a single file)'))}</span><input class="field" id="mh-name" value="${esc(m.name || '')}" spellcheck="false" placeholder="${esc(t('Keep its name'))}"></label>` : ''}
      ${m.how === 'arg' ? `<label class="mh-f"><span>${esc(t('Command line ({copy} is where your game files are)'))}</span><input class="field" id="mh-arg" value="${esc(m.arg || '{copy}')}" spellcheck="false"></label>` : ''}
    </div>
    <p class="err" id="mh-err" hidden></p>
    <div class="acts ${on ? 'grid3' : 'grid2'}">${on ? `<button type="button" class="btn" data-x="off">${esc(t('Turn It Off'))}</button>` : ''}<button type="button" class="btn" data-x="no">${esc(t('Cancel'))}</button><button type="button" class="btn main" data-x="save">${esc(t('Save'))}</button></div>`;
  const read = (box) => {
    const v = (sel) => { const el = $(sel, box); return el ? el.value.trim() : null; };
    if (v('#mh-into') != null) m.into = v('#mh-into');
    if (v('#mh-name') != null) m.name = v('#mh-name');
    if (v('#mh-arg') != null) m.arg = v('#mh-arg');
  };
  const scrim = modal(html(), async (x, close) => {
    const box = $('.modal', scrim);
    read(box);
    const redraw = () => { box.innerHTML = html(); };
    const err = (e) => { const el = $('#mh-err', box); if (el) { el.textContent = String(e && e.message ? e.message : e); el.hidden = false; } };
    switch (x.dataset.x) {
      case 'exe': { const p = await pickFile({ title: t('The program that starts the game'), program: true, defaultPath: g.path, directory: false, multiple: false }).catch(() => null); if (p) { m.exe = typeof p === 'string' ? p : p.path; redraw(); } return; }
      case 'copy-file': { const p = await pickFile({ title: t('Your game files'), directory: false, multiple: false }).catch(() => null); if (p) { m.copy = typeof p === 'string' ? p : p.path; redraw(); } return; }
      case 'copy-dir': { const p = await pickFile({ title: t('Your game files'), directory: true }).catch(() => null); if (p) { m.copy = typeof p === 'string' ? p : p.path; redraw(); } return; }
      case 'how': m.how = x.dataset.k; return redraw();
      case 'off':
        try { S.library[id] = await invoke('set_manual', { id, manual: null }); close(); toast(t('It uses the launcher\'s own setup again.')); render(true); } catch (e) { err(e); }
        return;
      case 'save':
        if (!m.exe) return err(t('Pick the program that starts the game.'));
        if (m.how !== 'none' && !m.copy) return err(t('Pick your game files, or choose It Asks for Them.'));
        try { S.library[id] = await invoke('set_manual', { id, manual: m }); close(); toast(t('Saved. Press Play to start it your way.')); render(true); } catch (e) { err(e); }
    }
  }, tf('Set Up {name} By Hand', { name }));
}

function actOn(id, a) { const el = document.createElement('i'); el.dataset.id = id; el.dataset.act = a; return act(el, new Event('click')); }
const TS_ICON = { ok: '<path d="m5 12.5 4.5 4.5L19 7.5"/>', warn: '<path d="M12 8v5M12 16.5v.01"/><path d="M10.3 3.9 2.6 17.5a2 2 0 0 0 1.7 3h15.4a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z"/>', bad: '<path d="m7 7 10 10M17 7 7 17"/>', info: '<circle cx="12" cy="12" r="9"/><path d="M12 11v5M12 8v.01"/>' };
async function troubleshootModal(id, lastCode) {
  // How it closed last time this session (for checks that only matter after that kind of exit).
  const raw = (S.lastExit || {})[id];
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
  const run = async () => { checks = null; draw(); try { checks = await invoke('troubleshoot', { id, code: Number.isFinite(Number(raw)) && raw !== undefined && raw !== null ? Number(raw) : null }); } catch (e) { checks = [{ level: 'bad', title: String(e && e.message ? e.message : e), detail: '', fix: '' }]; }
    if (lastCode) checks.push({ level: 'info', title: 'How it closed last time', detail: 'Code {code}', v: { code: lastCode }, fix: '' });
    // The player said they close it themselves with some codes: shown, and undone here.
    const quiet = ((S.settings.ui || {}).quiet_exits || {})[id] || [];
    if (quiet.length) checks.push({ level: 'info', title: N_('The crash window is off for some of its exits'), detail: N_('You said you closed it yourself when it closed with code {code}.'), v: { code: quiet.join(', ') }, fix: 'quiet' });
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

// Every cover, background and logo SteamGridDB has for the game, to pick from, and the player's
// own pictures from this PC. Another name can be looked up (a bit of it is enough).
function artPicker(shared, cur, own, opt, done) {
  // Art looked up here under another name is used only with Use These.
  const art = { covers: shared.covers || [], heroes: shared.heroes || [], logos: shared.logos || [] };
  let seq = 0;
  const sel = { cover: cur.cover, hero: cur.hero, logo: cur.logo };
  const yours = Object.assign({}, own);
  const KS = { cover: ['covers', N_('Covers ({n})')], hero: ['heroes', N_('Backgrounds ({n})')], logo: ['logos', N_('Logos ({n})')] };
  let tab = 'cover';
  const m = modal(`<h2>${esc(t('Choose Art'))}</h2>
    ${opt.key ? `<div class="ap-search"><input class="field" id="ap-q" type="search" value="${esc(opt.game() || '')}" spellcheck="false" placeholder="${esc(t('Look up another name…'))}" aria-label="${esc(t('Look up another name…'))}"></div>` : ''}
    <div class="seg ap-tabs" role="radiogroup" aria-label="${esc(t('Choose Art'))}">${Object.keys(KS).map((k) => `<button type="button" role="radio" data-x="tab" data-k="${k}"></button>`).join('')}</div>
    <div class="ap-grid" id="ap-grid"></div>
    <div class="acts grid3"><button type="button" class="btn" data-x="no">${esc(t('Cancel'))}</button><button type="button" class="btn" data-x="file">${esc(t('Your Picture…'))}</button><button type="button" class="btn main" data-x="use">${esc(t('Use These'))}</button></div>`, async (x, close) => {
    switch (x.dataset.x) {
      case 'tab': tab = x.dataset.k; return draw();
      case 'pick': sel[tab] = x.dataset.u; return draw();
      case 'mine': if (yours[tab]) sel[tab] = yours[tab].data; return draw();
      case 'file': {
        const p = await pickFile({ title: t('A picture'), directory: false, multiple: false, filters: [{ name: t('Pictures'), extensions: ['png', 'jpg', 'jpeg', 'webp', 'gif', 'bmp'] }] }).catch(fail);
        if (!p) return;
        try { yours[tab] = await window.ownArt.open(opt.id, tab, typeof p === 'string' ? p : p.path); sel[tab] = yours[tab].data; draw(); } catch (e) { fail(e); }
        return;
      }
      case 'use': {
        Object.assign(cur, sel);
        Object.assign(shared, art);
        for (const k of Object.keys(KS)) { if (yours[k] && sel[k] === yours[k].data) own[k] = yours[k]; else delete own[k]; }
        close(); done();
      }
    }
  }, t('Choose Art'));
  m.querySelector('.modal').classList.add('ap');
  function draw() {
    for (const b of $$('[data-x="tab"]', m)) { const k = b.dataset.k; b.textContent = tf(KS[k][1], { n: (art[KS[k][0]] || []).length + (yours[k] ? 1 : 0) }); b.setAttribute('aria-checked', String(k === tab)); }
    const list = art[KS[tab][0]] || [];
    const g = $('#ap-grid', m);
    g.className = 'ap-grid ' + tab;
    const n = list.length + (yours[tab] ? 1 : 0);
    g.style.setProperty('--cols', evenCols(n, tab === 'cover' ? [5, 4, 6, 3] : tab === 'logo' ? [4, 3, 5] : [2, 3]));
    const mineTile = yours[tab] ? `<button type="button" class="ap-it ap-mine${yours[tab].data === sel[tab] ? ' on' : ''}" data-x="mine" aria-pressed="${yours[tab].data === sel[tab]}">${pic(yours[tab].data)}<small>${esc(t('Yours'))}</small></button>` : '';
    g.innerHTML = n ? mineTile + list.map((x) => { const u = artUrl(x); return `<button type="button" class="ap-it${u === sel[tab] ? ' on' : ''}" data-x="pick" data-u="${esc(u)}" aria-pressed="${u === sel[tab]}">${pic(artThumb(x))}</button>`; }).join('')
      : `<p class="muted">${esc(opt.key ? t('SteamGridDB has none of these for this game. Look up another name, or use your own picture.') : t('Use your own picture, or add your own SteamGridDB key in Settings → Games and folders to get art.'))}</p>`;
  }
  // Another name: picked from SteamGridDB's list as you type, its art in place of what's shown.
  const q = $('#ap-q', m);
  if (q) suggestBox(q, gameFinder(opt.key, { sgdbOnly: true }), async (x) => {
    q.value = x.label;
    const g = $('#ap-grid', m), my = ++seq;
    g.innerHTML = `<p class="muted">${esc(t('Loading…'))}</p>`;
    try { const a = await invoke('art_for', { sgdb: x.sgdb, key: opt.key }); if (my !== seq) return; art.covers = a.covers || []; art.heroes = a.heroes || []; art.logos = a.logos || []; } catch (e) { if (my !== seq) return; fail(e); }
    if (m.isConnected) draw();
  });
  draw();
}

function editInfo(id) {
  const a = byId(id);
  if (!a || !a.custom) return;
  // The list entry's own values (not the player's pictures or name shown over them).
  const web = (k, u) => (a[k] !== undefined ? a[k] : u);
  gameInfoModal({ id, info: { name: web('_name', a.name), game: a.game, origin: a.origin, cover: web('_cov', a.cover), hero: web('_hero', a.hero), logo: web('_logo', a.logo) } });
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
  if (/^(acc|saves|myrep)-/.test(a) && window.accountAct) return window.accountAct(a, el);
  if (/^beta-/.test(a) && window.moreAct) return window.moreAct(a, el);
  if (a === 'project-association') return changeProject(id);
  if (a === 'adopt' && window.adoptAct) return window.adoptAct(a, el);
  if (/^mig-/.test(a) && window.migrateAct) return window.migrateAct(a, el);
  switch (a) {
    case 'open': e.preventDefault(); closeMenu(); return go('game', id, el.closest('.tile') ? el : null);
    case 'back': return back();
    case 'tab': return showTab(el.dataset.tab);
    case 'slide': return showSlide(Number(el.dataset.i));
    case 'slide-step': return showSlide((S.slide || 0) + Number(el.dataset.d));
    case 'shelf': { const sh = el.closest('.shelf'); S.shelf[sh.dataset.shelf] = (S.shelf[sh.dataset.shelf] || 0) + Number(el.dataset.d); return layoutShelves(sh.parentElement); }
    case 'sort': S.find.sort = el.dataset.k; S.find.picked = true; return updateDiscover();
    case 'clear-q': { S.q = ''; const top = el.closest('#dstick'); for (const id of ['dq', 'dqs']) { const q = $('#' + id); if (q) q.value = ''; } const q = $(top ? '#dqs' : '#dq'); if (q) q.focus(); return refreshList('dq'); }
    case 'ds-more': return stickPanel();
    case 'pin': { const p = (S.settings.ui || {}).pins || []; const on = p.includes(id); await saveUi({ pins: on ? p.filter((x) => x !== id) : [...p, id] }); return toast(on ? t('Unpinned.') : t('Pinned to the sidebar.')); }
    case 'fav': { const f = (S.settings.ui || {}).favs || []; const on = f.includes(id); await saveUi({ favs: on ? f.filter((x) => x !== id) : [...f, id] }); toast(on ? t('Removed from Favorites.') : t('Added to Favorites.')); return render(true); }
    case 'update-all': {
      // Every game with an update, one after another (downloads wait their turn).
      const ids = Object.keys(S.updates).filter((x) => S.library[x] && !S.running.has(x) && !S.jobs[x] && !(S.paused || {})[x]);
      for (const x of ids) startInstall(x);
      if (ids.length) toast(tf('Updating {n} games.', { n: ids.length }));
      return;
    }
    case 'auto-up': try { S.library[id] = await invoke('set_game_options', { id, args: null, autoUpdate: !S.library[id].auto_update }); toast(S.library[id].auto_update ? tf('{name} updates by itself now.', { name: S.library[id].name }) : t('Updates wait for you again.')); render(true); } catch (err) { fail(err); } return;
    case 'launch-opts': return launchOptions(id);
    case 'rename': return renameModal(id);
    case 'troubleshoot': return troubleshootModal(id);
    case 'edit-info': return editInfo(id);
    case 'proxy-save': return saveProxy('custom');
    case 'move-listing': return moveListing(id, el.dataset.to);
    case 'scan-open': return scanOpen(el);
    case 'dlss-remove': try { await invoke('dlss_remove'); S.dlssState = { applies: true, have: false }; toast(t('NVIDIA DLSS file removed.')); if (S.view === 'settings') setSection('library', false); } catch (e) { fail(e); } return;
    case 'umu-repair': try { toast(t(await invoke('umu_repair'))); S.protonNow = await invoke('proton_now').catch(() => S.protonNow); if (S.view === 'settings') setSection(S.setTab); } catch (e) { fail(e); } return;
    case 'sgdb-save': { const v = ($('#sgdb-key') || {}).value || ''; await saveUi({ sgdb_key: v.trim() }); return toast(v.trim() ? t('Saved. Games you add from GitHub get art now.') : t('Key removed.')); }
    case 'build': return changeBuild(id);
    case 'gh-add': return githubModal();
    case 'recomp-req': return requestModal();
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
    case 'hide': return hideGame(id);
    case 'unhide': toast(t('It shows in Discover again.')); return unhideGame(id);
    case 'hidden-list': return hiddenModal();
    case 'ui-toggle': return saveUi({ [el.dataset.k]: !(S.settings.ui || {})[el.dataset.k] });
    case 'dl-slow': return saveUi({ dl_slow_play: (S.settings.ui || {}).dl_slow_play === false }).then(sendLimits);
    case 'side-show': { const h = new Set((S.settings.ui || {}).side_hide || []); h.has(el.dataset.k) ? h.delete(el.dataset.k) : h.add(el.dataset.k); return saveUi({ side_hide: [...h] }); }
    case 'side-move': {
      const keys = sideSections().map((b) => b.dataset.go), i = keys.indexOf(el.dataset.k), j = i + Number(el.dataset.d);
      if (i < 0 || j < 0 || j >= keys.length) return;
      [keys[i], keys[j]] = [keys[j], keys[i]];
      return saveUi({ side_order: keys });
    }
    case 'side-reset': return saveUi({ side_order: [], side_hide: [] });
    case 'genre': e.preventDefault(); S.find.genre = el.dataset.g; if (S.view === 'discover') { S.anim = 'enter-soft'; render(true); } else go('discover'); return;
    case 'see-all': S.find.sort = el.dataset.sort; S.find.picked = false; S.find.genre = ''; S.q = ''; return go('discover');
    case 'mine': S.find.mine = !S.find.mine; return updateDiscover();
    case 'libf': S.lib.filter = el.dataset.f; return updateLibrary();
    case 'ver-menu': return verMenu(el);
    case 'follow': return toggleFollow(el.dataset.dev);
    case 'play-pick': { const a = byId(id); return menu(el, [a, ...otherInstalled(a)].map((v) => ({ act: 'play', id: v.id, label: tf('Play {name}', { name: v.name }), icon: 'play' }))); }
    case 'great': S.find.great = !S.find.great; return updateDiscover();
    case 'deck-f': S.find.deckgreat = !S.find.deckgreat; return updateDiscover();
    case 'notify': return toggleNotify(id);
    case 'note-x': return closeNote(el.dataset.n);
    case 'note-url': { const u = httpsOnly(el.dataset.url); return u ? invoke('open_url', { url: u }).catch(fail) : null; }
    case 'easy-f': S.find.easy = !S.find.easy; return updateDiscover();
    case 'mp-f': S.find.mp = !S.find.mp; return updateDiscover();
    case 'ended-f': S.find.hideEnded = !S.find.hideEnded; return updateDiscover();
    case 'ended-box': {
      S.endedOpen = !S.endedOpen;
      el.setAttribute('aria-expanded', String(S.endedOpen));
      if (el.nextElementSibling) el.nextElementSibling.classList.toggle('open', S.endedOpen);
      if (S.endedOpen) revealEnded(el);
      return;
    }
    case 'self-steam': return selfToSteam(false);
    case 'steam-sync': return steamSync(false);
    case 'ge-proton': return geInstall();
    case 'use-proton': return useProton(id);
    case 'readme': { S.readmeOpen = !S.readmeOpen; const c = el.closest('.rdm'); if (c) { c.classList.toggle('open', S.readmeOpen); el.setAttribute('aria-expanded', String(S.readmeOpen)); } return; }
    case 'run-say': return window.askRun ? window.askRun(id, (S.library[id] || {}).played_secs || 0, true) : null;
    case 'ver-go': if (S.view === 'game') return switchVersion(id); closeMenu(); return go('game', id);
    case 'ver-hide': { const p = saveUi({ verHide: !(S.settings.ui || {}).verHide }, true); refreshGame(); return p; }
    case 'install': return startInstall(id);
    case 'reinstall-new': return startInstall(id, null, null, true);
    case 'goback': return startInstall(id, undefined, el.dataset.tag);
    case 'wine-tools': return wineTools();
    case 'have-it': return haveIt(id);
    case 'update-all': for (const u of Object.keys(S.updates)) if (S.library[u] && !S.jobs[u] && !S.running.has(u) && !(S.paused || {})[u]) startInstall(u); return;
    case 'cancel': if ((S.paused || {})[id]) { delete S.paused[id]; render(true); } return invoke('cancel', { id });
    case 'dl-pause': return invoke('pause', { id });
    case 'dl-resume': return resumeDl(id);
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
    case 'bindcap': return bindAsk(id, el.dataset.k, Number(el.dataset.a), Number(el.dataset.i));
    case 'bindclear': { const o = bindingOf(id, el.dataset.k), a = o && o.bindings.actions[Number(el.dataset.a)]; if (a) bindSet(id, el.dataset.k, Number(el.dataset.a), a.keys.filter((_, j) => j !== Number(el.dataset.i))); return; }
    case 'bindreset': { try { await invoke('reset_game_bindings', { id, setting: el.dataset.k }); } catch (e) { fail(e); } delete S.setup[id]; return loadSetup(id); }
    case 'gadv': { await saveUi({ adv_open: Object.assign({}, (S.settings.ui || {}).adv_open || {}, { [id]: !advOpen(id) }) }, true); if (S.view === 'game' && S.arg === id && $('#g-body')) showTab(S.tab); else render(true); return; }
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
    case 'refresh': {
      // Once a minute at most: the list is the same a moment later.
      if (Date.now() - (S.refreshedAt || 0) < 60e3) { toast(t('Discover is up to date.')); return; }
      S.refreshedAt = Date.now();
      toast(t('Getting the newest list…')); await loadCatalog(true); await loadOwned(); loadEasy(); S.rels = {};
      toast(offNow() ? t('Can\'t reach recomplauncher.com right now, so this is the list saved on your PC.') : t('Discover is up to date.'));
      return render(true);
    }
    case 'tvsize': return saveSettings({ tv_size: el.dataset.k });
    case 'gfx-mode': if ((S.settings.linux_graphics || '') === el.dataset.k) return; await saveSettings({ linux_graphics: el.dataset.k }); return toast(t('Graphics Mode changes the next time the launcher opens.'));
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
    case 'customize': return window.customizeModal && window.customizeModal(id);
    case 'hero-reset': try { S.library[id] = await invoke('set_cover', { id, path: null, hero: true }); } catch (err) { return fail(err); } byId(id); return render(true);
    case 'start-page': { await saveUi({ start_page: el.dataset.k || '' }); return render(true); }
    case 'ui-scale': return setUiScale(Number(el.dataset.k));
    case 'theme': { const k = el.dataset.k; await saveUi({ theme: k === 'light' ? 'light' : '', oled: k === 'oled' }); return render(true); }
    case 'skip-backup': return invoke('skip_backup').catch(() => {});
    case 'check-upd': {
      el.disabled = true;
      // Once a minute at most asks the site; a second press within it reads what was just found.
      const fresh = Date.now() - (S.updCheckedAt || 0) >= 60e3;
      if (fresh) S.updCheckedAt = Date.now();
      const ok = await checkUpdates(fresh);
      el.disabled = false;
      if (ok && !Object.keys(S.updates || {}).length) toast(offNow() ? t('Can\'t reach recomplauncher.com right now. Try again in a bit.') : t('Everything is up to date.'));
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
    case 'rom-emus': return emuFolders();
    case 'found-fix': return fixFound(id);
    case 'copies': return copiesModal(id);
    case 'support': return supportModal(id);
    case 'tv': return window.enterTV && window.enterTV();
    case 'vcredist': return installVcredist();
    case 'report': S.reportGame = id; return setSection('report');
    case 'portable-open': return invoke('open_folder', { id: null, path: (S.portableInfo || {}).data }).catch(fail);
    case 'portable-make': {
      const f = await pickFile({ title: t('Where to put the portable copy'), directory: true }).catch(fail);
      const path = f && (typeof f === 'string' ? f : f.path);
      if (!path) return;
      try { const at = await invoke('make_portable', { target: path }); toast(tf('Portable copy made in {folder}. Start RecompLauncher from there.', { folder: at })); invoke('open_folder', { id: null, path: at }).catch(() => {}); } catch (err) { fail(err); }
      return;
    }
    case 'manual': return manualModal(id);
    case 'setexe': {
      const g = S.library[id];
      const p = await pickFile({ title: t('The program that starts the game'), program: true, defaultPath: g.path, directory: false, multiple: false, filters: S.os === 'windows' ? [{ name: t('Programs'), extensions: ['exe'] }] : [] }).catch(fail);
      if (!p) return;
      try { S.library[id] = await invoke('set_exe', { id, path: typeof p === 'string' ? p : p.path }); toast(t('Play starts that program now.')); render(true); } catch (err) { fail(err); }
      return;
    }
    case 'uninstall': return confirmUninstall(parentId(id));
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
      if (k === 'steam_sync') { await saveSettings({ steam_sync: !S.settings.steam_sync }); if (S.settings.steam_sync) steamSync(false); return; }
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
  if (!e.target.closest('.ds-more-w') && !e.target.closest('.dropmenu') && $('#dstick .ds-panel:not([hidden])')) stickPanel(false);
  const link = e.target.closest('[data-url]:not([data-act])');
  if (link) { e.preventDefault(); invoke('open_url', { url: link.dataset.url }).catch(fail); return; }
  const g = e.target.closest('[data-go]');
  if (g) { e.preventDefault(); if (['home', 'discover', 'soon', 'library', 'achievements', 'downloads', 'shots', 'saves', 'settings'].includes(g.dataset.go) || g.dataset.go.startsWith('plug:')) { if (g.closest('.side')) { if (g.dataset.go === 'discover') S.find.genre = ''; if (g.dataset.go === 'shots') S.shotsGame = ''; } go(g.dataset.go); } return; }
  const a = e.target.closest('[data-act]');
  if (a && a.tagName !== 'SELECT') { if (a.closest('.menu')) closeMenu(); act(a, e); }
});
document.addEventListener('change', (e) => {
  const s = e.target.closest('select[data-act]');
  if (!s) return;
  if (s.dataset.act === 'gsel') return setGameSetting(s.dataset.id, s.dataset.k, s.value);
  if (s.dataset.act === 'lang') return setLanguage(s.value);
  if (s.dataset.act === 'sort-sel') { S.find.sort = s.value; S.find.picked = true; return updateDiscover(); }
  if (s.dataset.act === 'origin') { S.find.origin = s.value; return updateDiscover(); }
  if (s.dataset.act === 'genre-sel') { S.find.genre = s.value; return updateDiscover(); }
  if (s.dataset.act === 'tag-sel') { S.find.tag = s.value; return render(true); }
  if (s.dataset.act === 'show-sel') { S.find.show = s.value; return updateDiscover(); }
  if (s.dataset.act === 'runs-sel') { setRuns(s.value).then(() => updateDiscover()); return; }
  if (s.dataset.act === 'libsort') { S.lib.sort = s.value; saveUi({ lib_sort: s.value }, true).catch(() => {}); return updateLibrary(); }
  if (s.dataset.act === 'liborigin') { S.lib.origin = s.value; return updateLibrary(); }
  render(true);
});
function focusSearch() {
  if (S.view !== 'discover' && S.view !== 'library') go('discover');
  setTimeout(() => { const q = $('#dstick.on #dqs') || $('#dq') || $('#lq'); if (q) { q.focus(); q.select(); } }, 60);
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
  else if (e.key === 'Escape') {
    if ($('.menu')) closeMenu();
    else if ($('#dstick .ds-panel:not([hidden])') && !$('.drop.open')) { stickPanel(false); const m = $('#dstick .ds-more'); if (m) m.focus(); }
    else if (e.target.id === 'dq' || e.target.id === 'dqs' || e.target.id === 'lq') { const id = e.target.id === 'lq' ? 'lq' : 'dq'; e.target.value = ''; if (id === 'dq') { S.q = ''; const o = $(e.target.id === 'dq' ? '#dqs' : '#dq'); if (o) o.value = ''; } else S.lq = ''; refreshList(id); e.target.blur(); }
  }
  else if (e.altKey && e.key === 'ArrowLeft') back();
});
document.addEventListener('mouseup', (e) => { if (e.button === 3) back(); });
// Typing in a search redraws only the list under it, so the field keeps its place.
function refreshList(which) {
  const box = which === 'dq' ? $('#dlist') : $('#llist');
  if (!box) return render(true);
  morphList(box, which === 'dq' ? discoverGrid() : libraryGrid());
  if (which === 'dq') { const h = $('.dhead h1'); if (h) h.textContent = S.q.trim() ? t('Search') : S.find.genre ? t(GENRES[S.find.genre]) : t('Discover'); const c = $('.bigsearch .clear'); if (c) c.hidden = !S.q.trim(); const c2 = $('#dstick .clear'); if (c2) c2.hidden = !S.q.trim(); }
}
let qTimer, tileTimer;
document.addEventListener('input', (e) => {
  const id = e.target.id;
  // The Library's tile size: live while dragging, saved once it settles.
  if (e.target.dataset && e.target.dataset.act === 'tile-size') {
    const v = Number(e.target.value);
    const l = $('#llist'); if (l) flipGrid(l, () => { l.style.setProperty('--tile', v); evenGrids(l); });
    // Kept at once (a redraw while dragging keeps it), saved once it settles.
    S.settings.ui = Object.assign({}, S.settings.ui, { tile: v });
    clearTimeout(tileTimer);
    tileTimer = setTimeout(() => saveUi({ tile: v }, true), 400);
    return;
  }
  // Typing in Home's search box: on to Discover's search, with what's been typed so far.
  if (id === 'hq') {
    S.q = e.target.value;
    go('discover');
    setTimeout(() => { const q = $('#dq'); if (q) { q.focus(); q.setSelectionRange(q.value.length, q.value.length); } }, 30);
    return;
  }
  if (id === 'dq' || id === 'dqs') { const o = $(id === 'dq' ? '#dqs' : '#dq'); if (o) o.value = e.target.value; }
  if (id === 'dqs') { clearTimeout(qTimer); qTimer = setTimeout(() => { S.q = e.target.value; refreshList('dq'); }, 120); return; }
  if (id !== 'dq' && id !== 'lq') return;
  clearTimeout(qTimer);
  qTimer = setTimeout(() => { if (id === 'dq') S.q = e.target.value; else S.lq = e.target.value; refreshList(id); }, 120);
});
$('#isl').addEventListener('click', () => go('downloads'));
// Past the top of a page, the window's buttons get a bar behind them.
$('#main').addEventListener('scroll', () => {
  const s = $('#main').scrollTop > 10; const bar = $('.topbar'); if (bar.classList.contains('solid') !== s) bar.classList.toggle('solid', s);
  if (S.view === 'discover') stickCheck();
  // While it scrolls, tiles passing under the pointer don't light up (that redraw is what made
  // scrolling stutter); they do again a moment after it stops. Marked on the page itself, not the
  // whole window: with hundreds of tiles, a mark on the window made every one of them be looked
  // at again each time a scroll started and stopped.
  const m = $('#main');
  if (!m.classList.contains('scrolling')) m.classList.add('scrolling');
  clearTimeout(scrollEnd.t); scrollEnd.t = setTimeout(scrollEnd, 160);
}, { passive: true });
function scrollEnd() { $('#main').classList.remove('scrolling'); }
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
  // A small copy that couldn't be had: the full picture instead, once.
  if (img.dataset.orig && img.getAttribute('src') !== img.dataset.orig) { img.setAttribute('src', img.dataset.orig); return; }
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
// A picture that couldn't be had (the site was busy, the connection dropped) is asked for again a
// little later, a few times, instead of leaving an empty tile until the page is drawn again.
const RETRY_AFTER = [5000, 30000, 75000, 150000];
document.addEventListener('error', (e) => {
  const im = e.target;
  if (!(im instanceof HTMLImageElement) || !im.isConnected) return;
  const src = im.getAttribute('src'), n = Number(im.dataset.tries || 0);
  if (!src || src.startsWith('data:') || n >= RETRY_AFTER.length) return;
  im.dataset.tries = String(n + 1);
  setTimeout(() => { if (im.isConnected && im.getAttribute('src') === src && !(im.naturalWidth > 0)) { im.loading = 'eager'; im.removeAttribute('src'); im.setAttribute('src', src); } }, RETRY_AFTER[n]);
}, true);
// Pictures in a part of the page that was just redrawn: one that's in but was never shown gets shown.
function reviveIn(root) {
  for (const im of (root || document).querySelectorAll('img')) {
    if (im.complete && im.naturalWidth > 0 && !im.classList.contains('ld')) { im.classList.add('ld'); LOADED.add(im.getAttribute('src')); }
  }
}
function reviveSoon(root) {
  requestAnimationFrame(() => reviveIn(root));
  setTimeout(() => reviveIn(root), 800);
}
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
// The same when the window comes back from being minimized or hidden in the tray.
document.addEventListener('visibilitychange', () => { if (!document.hidden) setTimeout(window.reviveImages, 300); });

// ---------- a controller on the desktop view ----------
// The D-pad or stick moves a highlight between what's on screen, A presses it, B goes back (or
// closes what's open), X opens a game's menu, LB and RB switch pages, and left and right change a
// list or a slider. Holding Menu still opens Big Picture mode. The mouse takes over when it moves.
const PAD = { el: null };
const PAD_ITEMS = 'button:not([disabled]), a[href], input:not([disabled]):not([type="hidden"]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';
function padLayer() {
  if (SU.el && SU.el.isConnected) return SU.el;
  const m = [...document.querySelectorAll('.menu')].pop();
  if (m) return m;
  return [...document.querySelectorAll('.scrim, .oob')].pop() || $('#app') || document.body;
}
function padTargets(root) {
  return [...root.querySelectorAll(PAD_ITEMS)].filter((el) => {
    // Never the window's own buttons (Close would close the launcher). Not the quick buttons on a
    // cover either (the cover itself is the one to pick), nor a list hidden behind its own button.
    if (/^w-/.test(el.id) || el.closest('[inert], [hidden], #tv, .qpw, [aria-hidden="true"]') || el.matches('.sel-hidden')) return false;
    const r = el.getBoundingClientRect();
    return r.width > 2 && r.height > 2 && getComputedStyle(el).visibility !== 'hidden';
  });
}
// Where the highlight starts: a dialog's main button, or the first thing on the page in view.
function padStart(root, list) {
  if (root.id !== 'app') return list.find((el) => el.matches('[aria-selected="true"], .btn.main, [data-x="yes"]')) || list[0];
  const main = $('#main'), box = main ? main.getBoundingClientRect() : null;
  const inView = list.filter((el) => main && main.contains(el) && el.getBoundingClientRect().top >= box.top - 4 && el.getBoundingClientRect().top < box.bottom);
  // A page's main button (Play) first, else the first thing at the top.
  return inView.find((el) => el.matches('.btn.main')) || inView.sort((a, b) => { const p = a.getBoundingClientRect(), q = b.getBoundingClientRect(); return (p.top - q.top) || (p.left - q.left); })[0] || list[0];
}
// The highlight put back on a control, without scrolling to it.
function padMark(el) {
  PAD.el = el;
  el.classList.add('pad-focus');
  if (document.activeElement !== el) el.focus({ preventScroll: true });
}
// A page or a list drawn again while the controller is in use keeps its highlight where it was.
new MutationObserver(() => {
  if (!document.body.classList.contains('pad-nav') || !PAD.el || PAD.el.isConnected || PAD.mending || PAD.lost === PAD.el) return;
  PAD.mending = true;
  requestAnimationFrame(() => {
    PAD.mending = false;
    if (!document.body.classList.contains('pad-nav') || !PAD.el || PAD.el.isConnected) return;
    const root = padLayer(), twin = PAD.sig && padTargets(root).find((el) => padSig(el) === PAD.sig);
    if (twin) padMark(twin); else PAD.lost = PAD.el;
  });
}).observe(document.body, { childList: true, subtree: true });
// What a control is, to find it again once the page is drawn anew.
const padSig = (el) => [el.tagName, el.id, el.dataset.act, el.dataset.id, el.dataset.f, el.dataset.k, el.dataset.go, el.getAttribute('aria-label')].join('|');
function padFocus(el) {
  if (!el) return;
  if (PAD.el && PAD.el !== el) PAD.el.classList.remove('pad-focus');
  PAD.el = el;
  PAD.sig = padSig(el);
  document.body.classList.add('pad-nav');
  el.classList.add('pad-focus');
  el.focus({ preventScroll: true });
  // Into view with room around it (its scroll margin), so the next row shows too. Gliding there,
  // unless the stick is held and the moves come quickly: then each one lands at once.
  const now = performance.now(), quick = now - (PAD.at || 0) < 160;
  PAD.at = now;
  el.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior: calm() || quick ? 'auto' : 'smooth' });
  if (window.pokeSlow) window.pokeSlow();
}
function padCurrent(root, list) {
  if (PAD.el && PAD.el.isConnected && root.contains(PAD.el) && list.includes(PAD.el)) return PAD.el;
  // Drawn anew under the highlight (a list refreshed): the same control in its new place.
  const twin = PAD.el && !PAD.el.isConnected && PAD.sig && list.find((el) => padSig(el) === PAD.sig);
  if (twin) { padMark(twin); return twin; }
  const a = document.activeElement;
  return a && a !== document.body && root.contains(a) && list.includes(a) ? a : null;
}
// The closest control that way: one in line (the same row or column) before one off to the side.
function padMove(dir) {
  const root = padLayer(), list = padTargets(root);
  if (!list.length) return;
  const cur = padCurrent(root, list);
  if (!cur) return padFocus(padStart(root, list));
  const side = dir === 'left' || dir === 'right';
  // Left and right change a list or a slider in place.
  if (side && cur.matches('select')) {
    const opts = [...cur.options], step = dir === 'right' ? 1 : -1;
    let i = cur.selectedIndex + step;
    while (opts[i] && opts[i].disabled) i += step;
    if (opts[i]) { cur.selectedIndex = i; cur.dispatchEvent(new Event('change', { bubbles: true })); }
    return;
  }
  if (side && cur.matches('input[type="range"]')) {
    if (cur.matches('[data-gslide]')) cur.dispatchEvent(new KeyboardEvent('keydown', { key: dir === 'right' ? 'ArrowRight' : 'ArrowLeft', bubbles: true }));
    else { if (dir === 'right') cur.stepUp(); else cur.stepDown(); cur.dispatchEvent(new Event('input', { bubbles: true })); cur.dispatchEvent(new Event('change', { bubbles: true })); }
    return;
  }
  const c = cur.getBoundingClientRect(), cx = c.left + c.width / 2, cy = c.top + c.height / 2;
  // First only what's wholly past its edge that way; when nothing is (things that overlap), what's
  // well over that way. Never something beside it in the same column (the sidebar's logo isn't
  // "left" of its buttons).
  const pick = (strict) => {
    let best = null, bestScore = Infinity;
    for (const el of list) {
      if (el === cur || el.contains(cur) || cur.contains(el)) continue;
      const r = el.getBoundingClientRect(), x = r.left + r.width / 2, y = r.top + r.height / 2;
      let gap, overlap, cross;
      if (side) {
        if (strict ? (dir === 'right' ? r.left < c.right - 6 : r.right > c.left + 6) : (dir === 'right' ? x <= cx + c.width / 2 : x >= cx - c.width / 2)) continue;
        gap = dir === 'right' ? r.left - c.right : c.left - r.right;
        overlap = Math.min(r.bottom, c.bottom) - Math.max(r.top, c.top);
        cross = Math.abs(y - cy);
      } else {
        if (strict ? (dir === 'down' ? r.top < c.bottom - 6 : r.bottom > c.top + 6) : (dir === 'down' ? y <= cy + c.height / 2 : y >= cy - c.height / 2)) continue;
        gap = dir === 'down' ? r.top - c.bottom : c.top - r.bottom;
        overlap = Math.min(r.right, c.right) - Math.max(r.left, c.left);
        cross = Math.abs(x - cx);
      }
      // Off to the side and far away isn't "that way" (the Sort list's right isn't the size slider
      // at the bottom of the window).
      if (overlap <= 0 && cross > Math.max(220, gap * 1.5 + 120)) continue;
      const score = Math.max(gap, -20) + (overlap > 0 ? cross * 0.3 : 400 + cross * 2);
      if (score < bestScore) { bestScore = score; best = el; }
    }
    return best;
  };
  const best = pick(true) || pick(false);
  if (best) padFocus(best);
}
// After a press that changes the page (or opens or closes a list or a dialog), the highlight
// follows as soon as there's something to put it on: what the page put focus on itself (a list
// that closed gives it back to its button), what opened it, else the new page's start.
function padAfter() {
  let tries = 0;
  const look = () => {
    if (!document.body.classList.contains('pad-nav')) return;
    const root = padLayer(), list = padTargets(root);
    if (PAD.el && PAD.el.isConnected && root.contains(PAD.el) && list.includes(PAD.el)) {
      // Still there: only a page that's still on its way in gets another look.
      if (++tries < 8) setTimeout(look, 70);
      return;
    }
    const a = document.activeElement;
    if (a && a !== document.body && list.includes(a)) return padFocus(a);
    if (PAD.from && PAD.from.isConnected && list.includes(PAD.from) && root.id === 'app') return padFocus(PAD.from);
    // The same page drawn again (a filter picked): the same control in its new place.
    const twin = PAD.sig && list.find((el) => padSig(el) === PAD.sig);
    if (twin) return padFocus(twin);
    if (list.length) return padFocus(padStart(root, list));
    if (++tries < 10) setTimeout(look, 70);
  };
  setTimeout(look, 60);
}
window.deskPad = (b) => {
  if (b === 'up' || b === 'down' || b === 'left' || b === 'right') { padMove(b); return true; }
  const root = padLayer(), list = padTargets(root), cur = padCurrent(root, list);
  if (b === 'a') {
    if (!cur) { padMove('down'); return true; }
    if (cur.matches('input[type="range"], select')) return true;
    if (cur.matches('input:not([type="checkbox"]):not([type="radio"]), textarea')) { cur.focus(); return true; }
    if (root.id === 'app') PAD.from = cur;
    cur.click();
    padAfter();
    return true;
  }
  if (b === 'b') {
    const over = [...document.querySelectorAll('.scrim, .oob')].pop();
    const layer = !!(SU.el || $('.menu') || over);
    if (SU.el) closeSetup();
    else if ($('.menu')) closeMenu();
    else if (over) over.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    else if (document.activeElement && document.activeElement.matches('input, textarea')) document.activeElement.blur();
    else if (S.back.length || S.view !== 'home') back();
    // Closing a menu or a dialog: back to what opened it.
    if (layer && PAD.from && PAD.from.isConnected && !$('.menu') && !SU.el) setTimeout(() => { if (PAD.from && PAD.from.isConnected && padLayer().contains(PAD.from)) padFocus(PAD.from); }, 30);
    padAfter();
    return true;
  }
  if (b === 'lb' || b === 'rb') {
    const navs = $$('#side .nav[data-go]').filter((n) => n.offsetParent !== null);
    const i = navs.findIndex((n) => n.dataset.go === S.view);
    const next = navs[(i < 0 ? 0 : i + (b === 'rb' ? 1 : -1) + navs.length) % navs.length];
    if (next) { next.click(); padAfter(); }
    return true;
  }
  if (b === 'x' && cur) {
    const g = cur.closest('[data-id]');
    if (g) {
      PAD.from = cur;
      const r = cur.getBoundingClientRect();
      cur.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: r.left + r.width / 2, clientY: r.top + r.height / 2 }));
      setTimeout(() => { const m = $('.menu'); if (m) padFocus(padTargets(m)[0]); }, 60);
    }
    return true;
  }
  return false;
};
// The mouse moving again: no highlight.
document.addEventListener('mousemove', (e) => {
  if (!document.body.classList.contains('pad-nav') || (Math.abs(e.movementX) + Math.abs(e.movementY) < 3)) return;
  document.body.classList.remove('pad-nav');
  if (PAD.el) PAD.el.classList.remove('pad-focus');
  PAD.el = null;
}, true);

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
  // Paused: stopped where it got to, shown with Resume until the player carries on or cancels.
  if (p.phase === 'paused') {
    const j = S.jobs[id] || {};
    delete S.jobs[id];
    (S.paused = S.paused || {})[id] = { id, phase: 'paused', done: j.done || 0, total: j.total || 0 };
    refreshState().then(() => render(true));
    return;
  }
  if (['done', 'error', 'cancelled'].includes(p.phase)) {
    if (S.paused) delete S.paused[id];
    delete S.jobs[id];
    delete DLSPD[id];
    ENDED[id] = Date.now();
    S.recent = [p].concat(S.recent.filter((r) => r.id !== id)).slice(0, 20);
    if (p.phase === 'done') S.dlHist = null;
    const name = byId(id)?.name || id;
    if (p.phase === 'done') {
      delete S.updates[id]; delete S.mods[id]; delete S.setup[id]; loadNews();
      // A game the launcher sets up: straight on to its copy, unless it already has one.
      const busy = SU.el || (window.tvSetupBusy && window.tvSetupBusy());
      // A copy picked (and checked) before the download: its setup starts with it at once.
      const pre = (S.prePicked || {})[id];
      if (pre) delete S.prePicked[id];
      // Only an installer: its Set Up opens to run it (an update's too).
      if (isInstallerApp(byId(id))) { if (!busy && !document.body.classList.contains('tv-on')) refreshState().then(() => { if (installerDue(id)) installerModal(id); }); else toast(tf('{name} is downloaded. Press Set Up to run its installer.', { name })); }
      else if (S.easy.has(id) && !busy) loadSetup(id).then(async () => { const su = S.setup[id]; if (su && su.needs && !su.copy) { await setupModal(id); if (pre && SU.el && SU.id === id && SU.stage === 'pick') suBegin(pre); } else toast(tf('{name} is ready to play.', { name })); });
      else if (S.easy.has(id)) toast(tf('{name} is installed. Set it up from its page.', { name }));
      else toast(tf('{name} is ready to play.', { name }));
    }
    else if (p.phase === 'error' && String(p.msg || '').startsWith('PICK_BUILD|')) { S.recent = S.recent.filter((r) => r !== p); pickBuild(id, buildList(p.msg)); }
    else if (p.phase === 'error') { if (!blocked(id, p.msg)) toast(tf('{name}: {msg}', { name, msg: cleanMsg(p.msg) }), true); p.msg = cleanMsg(p.msg); }
    refreshState().then(() => render(true));
    return;
  }
  const had = S.jobs[id];
  p.at = Date.now();
  S.jobs[id] = p;
  if (p.phase === 'download') noteSpeed(id, p);
  if (!had || had.phase !== p.phase) render(true); else { paintProgress(id); paintDlHero(id); paintIsland(); }
});

T.event.listen('game', ({ payload: p }) => {
  if (p.state === 'running') { S.running.add(p.id); if (p.game) S.running.add(p.game); document.body.classList.add('playing'); }
  else {
    S.running.delete(p.id);
    if (p.game) S.running.delete(p.game);
    // 0xC000013A: its console window was closed, which is how some games are quit.
    if (p.code != null && p.code !== 0 && p.code !== -1073741510 && p.code !== 3221225786) crashModal(p);
    delete S.shots[p.id];
    // ReShade may have been downloaded for it, and there may be new screenshots to preview on.
    forgetLooks(p.id);
    // Its own menu may have changed its settings while it ran: read again (nothing is written).
    if (S.setup[p.id] && S.setup[p.id] !== 'loading') { delete S.setup[p.id]; loadSetup(p.id); }
  }
  refreshState().then(() => render(true));
});

// A recomp's own installer or first-time setup, opened by its first Play.
// Its own setup finished, but the game it made wasn't found: the player points at it.
// After a game's own setup: which program starts the game. The launcher's pick is first; the
// others are programs the setup just made. Pick Another for anything else.
function confirmProgram(id, name, list, ask) {
  let sel = 0;
  const dirOf = (p) => p.replace(/[\\/][^\\/]*$/, '');
  const fileOf = (p) => p.split(/[\\/]/).pop();
  // Programs named like tools (a translator, a patcher, a builder) go below the rest, so the first
  // one, picked already, is the likeliest game.
  const tool = (p) => (/xlate|translat|tool|patch|setup|install|extract|unpack|build|convert|config|crash|unins|redist|shader|compil|deps|server/i.test(fileOf(p)) || /[\\/](deps|tools?|temp|tmp|compilers?|sdk)[\\/]/i.test(p) ? 1 : 0);
  list = [...list].sort((x, y) => tool(x) - tool(y));
  const m = modal(`<h2>${esc(ask ? tf('Which Program Starts {name}?', { name }) : tf('{name} Is Set Up', { name }))}</h2><p>${esc(ask ? t('Its download has more than one program, and the ones the launcher would pick look like tools. Choose the game, and Play starts it from then on.') : t('Which program starts the game? Its setup may have left other programs too, like a builder or a tool, so check this one.'))}</p>
    <div class="cp-list" role="radiogroup">${list.map((p, i) => `<button type="button" class="cp-it${i === 0 ? ' on' : ''}" role="radio" aria-checked="${i === 0}" data-x="cp" data-i="${i}"><b>${esc(fileOf(p))}</b><small dir="ltr">${esc(dirOf(p))}</small></button>`).join('')}</div>
    <div class="acts grid2"><button type="button" class="btn" data-x="other">${esc(t('Pick Another…'))}</button><button type="button" class="btn main" data-x="yes">${esc(t('Play Starts This One'))}</button></div>`, async (x, close) => {
    if (x.dataset.x === 'cp') { sel = Number(x.dataset.i); for (const b of $$('.cp-it', m)) { const on = Number(b.dataset.i) === sel; b.classList.toggle('on', on); b.setAttribute('aria-checked', String(on)); } return; }
    if (x.dataset.x === 'yes') {
      try { S.library[id] = await invoke('set_exe', { id, path: list[sel] }); close(); render(true); if (ask) playGame(id); else toast(tf('{name} is set up. Press Play to start it.', { name })); } catch (e) { fail(e); }
      return;
    }
    if (x.dataset.x === 'other') {
      const g = S.library[id] || {};
      const f = await pickFile({ title: t('The program that starts the game'), program: true, defaultPath: dirOf(list[sel]) || g.path, directory: false, multiple: false, filters: S.os === 'windows' ? [{ name: t('Programs'), extensions: ['exe'] }] : [] }).catch(fail);
      if (!f) return;
      try { S.library[id] = await invoke('set_exe', { id, path: typeof f === 'string' ? f : f.path }); close(); toast(t('Play starts that program now.')); render(true); } catch (e) { fail(e); }
    }
  }, t('Find the game'));
}
function lostModal(id, name) {
  const a = byId(id) || {};
  const file = (((a.boot || {}).installer || {}).then_start || '').split(/[\\/]/).pop();
  modal(`<h2>${esc(tf('Where Did You Install {name}?', { name }))}</h2>
    <p>${esc(file ? tf('Its setup is done, but the launcher couldn\'t find the game. Choose {file} in the folder you installed it to, and Play starts it from then on.', { file }) : t('Its setup is done, but the launcher couldn\'t find the game. Choose its program in the folder you installed it to, and Play starts it from then on.'))}</p>
    <div class="acts grid2"><button type="button" class="btn" data-x="no">${esc(t('Not Now'))}</button><button type="button" class="btn main" data-x="pick">${esc(t('Choose the Program'))}</button></div>`, async (x, close) => {
    close();
    if (x.dataset.x !== 'pick') return;
    const g = S.library[id] || {};
    const f = await pickFile({ title: t('The program that starts the game'), program: true, defaultPath: g.path, directory: false, multiple: false, filters: S.os === 'windows' ? [{ name: t('Programs'), extensions: ['exe'] }] : [] }).catch(fail);
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
    else if (p.state === 'confirm' && Array.isArray(p.candidates) && p.candidates.length) confirmProgram(p.id, name, p.candidates);
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
// A game link from the website (recomplauncher://game/<id>): that game's page, once the catalog is
// in and the first-time setup is done. A game the catalog doesn't have: Discover, with a word on why.
// Only the page opens: installing and playing stay the player's own presses.
let linkWait = 0;
function openLinkedGame(id) {
  id = String(id || '');
  if (!/^[A-Za-z0-9-]{1,80}$/.test(id)) return;
  invoke('take_link').catch(() => {});
  clearTimeout(linkWait);
  const started = Date.now();
  let asked = false;
  const tryOpen = async () => {
    const ready = S.cat && S.cat.apps && S.cat.apps.length && !document.querySelector('.oob');
    if (!ready && Date.now() - started < 120000) { linkWait = setTimeout(tryOpen, 400); return; }
    const tv = document.body.classList.contains('tv-on') && window.tvGoto;
    const known = () => (S.cat && byId(id)) || S.library[id];
    // Not in the list on disk: it may be newer than that list. Asked for once, then looked at again.
    if (!known() && !asked) {
      asked = true;
      await loadCatalog(true);
      render(true);
      if (window.tvRefresh) window.tvRefresh();
    }
    if (known()) { if (tv) window.tvGoto('game', id); else go('game', id); return; }
    if (tv) window.tvGoto('discover'); else go('discover');
    toast(t('That game isn\'t in RecompLauncher yet'), true);
  };
  tryOpen();
}
T.event.listen('open-game', ({ payload }) => openLinkedGame(payload));
// Something on its way that takes a while (a Flatpak installing before its game starts).
T.event.listen('note', ({ payload }) => toast(t(String(payload)), false, null, 9000));
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
T.event.listen('away', ({ payload: away }) => { S.away = !!away; document.body.classList.toggle('away', S.away); });
T.event.listen('new-recomp', () => loadOwned().then(() => render(true)));
T.event.listen('screenshot', ({ payload: p }) => { delete S.shots[p.id]; forgetLooks(p.id); if (S.view === 'game' && S.arg === p.id) loadShots(p.id); });

(async function start() {
  let loaded = true;
  try { await refreshState(); } catch (e) { loaded = false; fail(e); }
  // Settings that didn't load are never saved over (they'd be lost).
  if (loaded) { try { sideOnce(); } catch (e) { /* the sidebar keeps its order */ } sendLimits(); }
  const lang = (S.settings.ui || {}).lang;
  await loadLang(lang && lang !== 'auto' ? lang : systemLang());
  tDom();
  if ((S.settings.ui || {}).start_page === 'library' && Object.keys(S.library).length) S.view = 'library';
  else if ((S.settings.ui || {}).start_page === 'discover') S.view = 'discover';
  // The Library's sort, as the player left it.
  if (['played', 'time', 'az'].includes((S.settings.ui || {}).lib_sort)) S.lib.sort = S.settings.ui.lib_sort;
  render();
  trayWords();
  await loadCatalog(false);
  // Both read the list on disk: side by side.
  await Promise.all([loadOwned(), loadEasy(true)]);
  // No connection and nothing saved from before: the Library still works.
  if (S.offline && !S.cat.apps.length && Object.keys(S.library).length) S.view = 'library';
  // The page the player picked to open on (Settings → Appearance), when nothing else was opened yet.
  if ((S.settings.ui || {}).start_page === 'library' && S.view === 'home' && Object.keys(S.library).length) S.view = 'library';
  // Discover with nothing to show (offline, nothing saved): the Library, as above.
  if (S.view === 'discover' && S.offline && !S.cat.apps.length && Object.keys(S.library).length) S.view = 'library';
  S.anim = 'enter';
  render(true);
  // Update check first (it keeps the release info on disk current), then the news reads from it.
  const upd = S.settings.check_updates !== false ? checkUpdates() : Promise.resolve();
  applyLighter();
  watchSlow();
  // Handheld or not is worked out in the background when the launcher starts.
  setTimeout(() => invoke('is_handheld').then((h) => { S.handheld = !!h; applyLighter(); if (S.view === 'settings') render(); }).catch(() => {}), 3000);
  invoke('pc_check').then((pc) => { S.pc = pc; if (S.view === 'settings') render(true); }).catch(() => {});
  loadNotes();
  loadLibs();
  lookForLauncher(true);
  upd.finally(() => loadNews());
  // The first time: the setup, like a new console. After that, TV mode if that's how they play.
  if (document.readyState === 'loading') await new Promise((ok) => addEventListener('DOMContentLoaded', ok, { once: true }));
  if (!S.settings.setup_done && window.firstRun) window.firstRun();
  else if (window.startTV && startsInTV()) window.startTV();
  else setTimeout(askData, 800);
  // The window opens only now, with the first real screen already drawn (two frames: the draw, then its paint).
  // A hidden window may not get frames at all, so a short timer opens it too, whichever comes first;
  // waiting for the launcher's 4 s fallback on every start would be worse than an early show.
  let shown = false;
  const ready = () => { if (!shown) { shown = true; invoke('ui_ready').catch(() => {}); } };
  requestAnimationFrame(() => requestAnimationFrame(ready));
  setTimeout(ready, 150);
  // Started by a game link from the website: that game's page.
  invoke('take_link').then((id) => { if (id) setTimeout(() => openLinkedGame(id), window.startTV && startsInTV() ? 1600 : 0); }).catch(() => {});
  // A shortcut asked to play a recomp that still needs setting up: its Set Up window.
  invoke('take_setup_asked').then((id) => { if (id) setTimeout(() => setupReqModal(id), window.startTV && startsInTV() ? 1600 : 0); }).catch(() => {});
})();
