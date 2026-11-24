// Recomps that carry several games (a whole series in one release). Each game gets its own tile,
// page, play time and achievements under its full id (<recomp id>--<game id>), while the install,
// updates and removal stay the recomp's. The core lists the installed ones' games (recomp_games:
// the developer's list wins over the catalog's); for the rest, the catalog entry's list is used.
// One game, or none: the recomp as before.

const GAME_SEP = '--';
const gameIdOk = (id) => typeof id === 'string' && /^[a-z0-9]{1,16}$/.test(id);

// A list as the launcher uses it: bad and repeated ids left out, fewer than two is none.
function gameList(list) {
  const seen = new Set(), out = [];
  for (const g of Array.isArray(list) ? list : []) {
    if (!g || !gameIdOk(g.id) || seen.has(g.id) || !String(g.name || '').trim()) continue;
    seen.add(g.id);
    out.push(g);
  }
  return out.length < 2 ? [] : out;
}

// A recomp's games: the core's (installed), else the catalog entry's. [] for one that has none.
function gamesOf(rid, entry) {
  const set = (S.gameSets || {})[rid];
  if (set && set.games && set.games.length) return set.games;
  return gameList(entry && entry.games);
}

// A full id taken apart, when it's one of a recomp's games: { rid, game, entry }.
function subOf(id) {
  return (S.subs || {})[id] || null;
}
const parentId = (id) => { const s = subOf(id); return s ? s.rid : id; };

// One game's pictures of a kind, game and project (app.js pickArt picks between them): the game's,
// else the recomp's. Its project picture only when the game has no picture of its own, or the
// recomp's one project cover would make every game of a series look the same.
function childArt(c, a, g, k) {
  const pk = 'project_' + k;
  c[k] = g[k] || gameArt(a, k);
  c[pk] = g[pk] || (g[k] ? '' : a[pk] || '');
}

// One game as an app the pages and tiles can show: the recomp's details, the game's name and art
// (falling back to the recomp's). Not part of the version groups; Discover lists it through withGames.
function childApp(a, g) {
  const c = Object.assign({}, a, {
    id: a.id + GAME_SEP + g.id, name: g.name, game: g.name, parent: a.id, parentName: a.name, sub: g.id,
    menuPick: !(Array.isArray(g.args) && g.args.length),
  });
  for (const k of ['cover', 'hero', 'logo', 'icon']) childArt(c, a, g, k);
  // The recomp's game info is about one game of them (Red's for all of Gen 1): a game's page asks
  // for its own instead, unless the catalog gives it one.
  if (g.about) c.about = g.about; else delete c.about;
  delete c.title; delete c.games; delete c._name; delete c._cov; delete c._hero; delete c._logo; delete c._icon;
  return c;
}

// The games of every recomp with several, in the catalog's index: (full id -> app), and S.subs.
function addGameApps(ids) {
  const subs = {};
  for (const a of [...ids.values()]) {
    for (const g of gamesOf(a.id, a)) {
      const c = childApp(a, g);
      ids.set(c.id, c);
      subs[c.id] = { rid: a.id, game: g };
    }
  }
  // Installed ones the catalog doesn't list (added by the player): from the core's list.
  for (const [rid, set] of Object.entries(S.gameSets || {})) {
    if (ids.has(rid) || !S.library[rid]) continue;
    const g0 = S.library[rid];
    const base = { id: rid, name: g0.name, repo: g0.repo, systems: [S.os], cover: '', hero: '', logo: '', icon: '' };
    for (const g of set.games || []) { const c = childApp(base, g); ids.set(c.id, c); subs[c.id] = { rid, game: g }; }
  }
  S.subs = subs;
}

// The games of a recomp that carries several, as Discover lists them: the ones not hidden.
function shownGames(a) {
  return gamesOf(a.id, a).map((g) => byId(a.id + GAME_SEP + g.id)).filter((c) => c && !isHidden(c.id));
}
// Discover's list as a player picks from it: a recomp that carries several games shows as those
// games, each a store entry of its own. Its own page is one press away, from their "Part of" pill.
function withGames(list) {
  return list.flatMap((a) => (a.parent || !gamesOf(a.id, a).length ? [a] : shownGames(a)));
}
// How many recomps a list holds: the games of one count once, as it.
const recompsIn = (list) => new Set(list.filter((a) => !a.soon).map((a) => a.parent || a.id)).size;

// Each installed recomp with several games: a record for each game in the Library (its own play
// time and last played), beside the recomp's own (which the lists leave out). Answers the core
// gave for a game's id (they're the recomp's) go back on the recomp.
function expandLib() {
  const lib = S.library;
  for (const [k, g] of Object.entries(lib)) {
    if (!g || g._sub) { delete lib[k]; continue; }
    if (g.id && g.id !== k && k.includes(GAME_SEP) && k.startsWith(g.id + GAME_SEP)) { lib[g.id] = g; delete lib[k]; }
  }
  for (const [rid, set] of Object.entries(S.gameSets || {})) {
    const p = lib[rid];
    if (!p) continue;
    for (const g of set.games || []) {
      const mine = (p.games || {})[g.id] || {};
      lib[rid + GAME_SEP + g.id] = Object.assign({}, p, {
        id: rid + GAME_SEP + g.id, name: g.name, game: g.name, parent: rid, sub: g.id, _sub: true,
        played_secs: mine.played_secs || 0, last_played: mine.last_played || 0, sessions: mine.sessions || [],
        size: 0, cover_file: null, hero_file: null, logo_file: null, icon_file: null,
        ready: !set.ready || set.ready[g.id] !== false, menuPick: !(Array.isArray(g.args) && g.args.length),
      });
    }
  }
}

// A recomp with several games installed: its games stand in for it in the lists.
const hasGames = (id) => !!((S.gameSets || {})[id] && S.gameSets[id].games && S.gameSets[id].games.length && S.library[id] && !S.library[id]._sub);
// The Library's records as the lists show them: each game of a recomp, not the recomp itself.
const libRecs = () => Object.values(S.library).filter((g) => !hasGames(g.id));
// The game of a recomp the player installed it from (its full id): the one whose setup opens after
// the download, and the one Jump back in shows of it besides those played.
const packPick = (rid) => (((S.settings || {}).ui || {}).pack_from || {})[rid] || '';
// Jump back in and Continue playing: installing a recomp of several games doesn't put all of them
// there. Its games that were played are, and the one it was installed from (else its first, until
// one is played).
function jumpRecs() {
  const recs = libRecs();
  const played = new Set(recs.filter((g) => g._sub && g.last_played).map((g) => g.parent));
  const first = {};
  for (const g of recs) if (g._sub && !(g.parent in first)) first[g.parent] = g.id;
  return recs.filter((g) => {
    if (!g._sub || g.last_played) return true;
    const pick = packPick(g.parent);
    if (pick) return g.id === pick;
    return !played.has(g.parent) && g.id === first[g.parent];
  });
}
// The core's own records only (one per install): for sizes and counts of installs.
const libInstalls = () => Object.values(S.library).filter((g) => !g._sub);

// This PC's own time for a recomp (or one of its games), installed or kept from an uninstall.
function localPlaytime(id) {
  if (S.library[id]) return S.library[id];
  const removed = S.removedPlayHistory || {};
  const sub = subOf(id);
  return sub ? removed[sub.rid]?.games?.[sub.game.id] : removed[id];
}

// The account's total over all its PCs, when signed in and the website has one ({secs, last}).
// It already counts this PC, but may not have this PC's newest session yet: the larger one wins.
function withAccount(play, cloud) {
  if (!cloud) return play;
  const secs = Math.max(Number(cloud.secs) || 0, play?.played_secs || 0);
  const last = Math.max(Number(cloud.last) || 0, play?.last_played || 0);
  return { played_secs: secs, last_played: last };
}

function recompPlaytime(id) {
  return withAccount(localPlaytime(id), S.cloudPlay?.games?.[id]);
}

// One session as [start, secs], both whole seconds above zero; anything else from the website is
// left out rather than drawn.
const sessionOk = (x) => Array.isArray(x) && Number.isInteger(x[0]) && x[0] > 0 && Number.isInteger(x[1]) && x[1] > 0;
// A whole number from the website, or 0.
const wholeOr0 = (n) => (Number.isInteger(n) && n > 0 ? n : 0);
// How far back the card looks (as PLAY_RECENT_SECS in library.rs): a month, plus a day.
const PLAY_RECENT_DAYS = 31;

// A game's play time card over every PC of the account: how many sessions and the longest, and
// the recent sessions (oldest first) for this week, this month and the bars. The account's
// already hold this PC's as the same [start, secs], so those two numbers join them, and the
// larger count and longest win: this PC's newest session counts before the website has it.
// Signed out, or a website that can't say, it's this PC's own.
function playStats(id, own) {
  const mine = (own || []).filter(sessionOk);
  const cloud = S.cloudPlay?.games?.[id] || {};
  const from = Date.now() / 1000 - PLAY_RECENT_DAYS * 86400;
  const byKey = new Map();
  for (const x of [...mine, ...(Array.isArray(cloud.recent) ? cloud.recent : [])]) {
    if (!sessionOk(x) || x[0] < from) continue;
    byKey.set(`${x[0]}|${x[1]}`, [x[0], x[1]]);
  }
  const recent = [...byKey.values()].sort((a, b) => a[0] - b[0]);
  const lengths = [...mine, ...recent].map((x) => x[1]);
  const count = Math.max(mine.length, recent.length, wholeOr0(cloud.count));
  const longest = Math.max(0, ...lengths, wholeOr0(cloud.longest));
  return { recent, count, longest };
}

// A recomp's time, or the original game's over all its recomps (marked `grouped`).
function gamePlaytime(a) {
  if (!a) return;
  const own = recompPlaytime(a.id);
  const group = a.title && index().titles.get(a.title);
  if (!group || gamesOf(a.id, a).length) return own;

  let total = { played_secs: 0, last_played: 0 };
  for (const version of group.every) {
    if (gamesOf(version.id, version).length) continue;
    const play = recompPlaytime(version.id);
    if (!play) continue;
    total.played_secs += play.played_secs || 0;
    total.last_played = Math.max(total.last_played, play.last_played || 0);
  }
  total = withAccount(total, S.cloudPlay?.titles?.[a.title]);
  return Object.assign(total, { grouped: true });
}

function playtimeRows(a, total) {
  if (!total?.grouped) return [[t('Played'), playedFor(total)]];
  return [[t('Total game playtime'), playedFor(total)], [t('This recomp'), playedFor(recompPlaytime(a.id))]];
}

// What the core is asked with a game's id. Most things are the recomp's (its install, updates,
// folder, settings): those get the recomp's id. Playing it, its setup, shortcuts and achievements
// go by the game's own id, and so do its saves when it keeps its own.
const GAME_CMDS = new Set(['play', 'game_info', 'game_setup', 'setup_inputs_run', 'desktop_shortcut', 'add_to_steam', 'ach_game', 'ach_set_disc', 'ach_set_folder']);
const SAVE_CMDS = new Set(['saves_check', 'saves_pick', 'saves_play_without_cloud', 'saves_info', 'saves_status', 'saves_peek', 'saves_upload_now', 'saves_backup', 'saves_restore', 'saves_cloud_delete', 'saves_open', 'saves_open_backups']);
function gameArgs(cmd, args) {
  if (!args || typeof args.id !== 'string') return args;
  const s = subOf(args.id);
  if (!s) return args;
  if (GAME_CMDS.has(cmd)) return args;
  if (SAVE_CMDS.has(cmd) && Array.isArray(s.game.saves) && s.game.saves.length) return args;
  return Object.assign({}, args, { id: s.rid });
}

// The core's list of installed recomps' games.
async function loadGameSets() {
  try { S.gameSets = (await invoke('recomp_games')) || {}; } catch (e) { S.gameSets = S.gameSets || {}; }
  expandLib();
}

if (typeof window !== 'undefined') Object.assign(window, { gameList, gamesOf, subOf, parentId, childApp, addGameApps, expandLib, hasGames, libRecs, jumpRecs, packPick, libInstalls, gameArgs, loadGameSets });
