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

// One game as an app the pages and tiles can show: the recomp's details, the game's name and art
// (falling back to the recomp's). Not part of the version groups or Discover's list.
function childApp(a, g) {
  const c = Object.assign({}, a, {
    id: a.id + GAME_SEP + g.id, name: g.name, game: g.name, parent: a.id, parentName: a.name, sub: g.id,
    cover: g.cover || a.cover, hero: g.hero || a.hero, logo: g.logo || a.logo, icon: g.icon || a.icon,
    menuPick: !(Array.isArray(g.args) && g.args.length),
  });
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
// The core's own records only (one per install): for sizes and counts of installs.
const libInstalls = () => Object.values(S.library).filter((g) => !g._sub);

// What the core is asked with a game's id. Most things are the recomp's (its install, updates,
// folder, settings): those get the recomp's id. Playing it, its setup, shortcuts and achievements
// go by the game's own id, and so do its saves when it keeps its own.
const GAME_CMDS = new Set(['play', 'game_setup', 'setup_inputs_run', 'desktop_shortcut', 'add_to_steam', 'ach_game', 'ach_set_disc']);
const SAVE_CMDS = new Set(['saves_check', 'saves_pick', 'saves_info', 'saves_upload_now', 'saves_backup', 'saves_restore', 'saves_open', 'saves_open_backups']);
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

if (typeof window !== 'undefined') Object.assign(window, { gameList, gamesOf, subOf, parentId, childApp, addGameApps, expandLib, hasGames, libRecs, libInstalls, gameArgs, loadGameSets });
