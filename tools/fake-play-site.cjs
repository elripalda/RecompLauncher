#!/usr/bin/env node
// A stand-in for recomplauncher.com, for trying playtime sync before the website has it.
//
//   node tools/fake-play-site.cjs [--port 8787] [--no-play]
//
// Then start the launcher with RL_API=http://<this machine>:8787 and sign in from Settings,
// Account: it signs in straight away to one fake account, with each launcher as its own PC.
// The two playtime calls (docs/LAUNCHER-API.md, "Playtime") work as the website should do them.
// Every other call made with a fake token gets an empty answer, so cloud saves, the saved list and
// the inbox stay quiet. Anything else (the list of games, pictures) comes from the real site.
//
// --no-play answers the playtime calls the way the real site does before it has them (401), to
// check the launcher falls back to the PC's own numbers and stays signed in.
// GET /state shows what's stored. Nothing is kept after it stops.

const http = require('node:http');
const https = require('node:https');

const REAL = 'https://recomplauncher.com';
const args = process.argv.slice(2);
const portAt = args.indexOf('--port');
const PORT = portAt >= 0 ? Number(args[portAt + 1]) : 8787;
const NO_PLAY = args.includes('--no-play');

const ACCOUNT = { uid: 'fake-1', username: 'Playtime tester', avatar: '' };
// token -> PC name, poll code -> PC name, and the playtime rows: "pc|game" -> {pc, game, secs, last}.
const pcs = new Map();
const polls = new Map();
const rows = new Map();
// The catalog's grouping by original game: recomp id -> title id.
let titleOf = new Map();
const MAX_SECS = 20 * 365 * 24 * 3600;

function send(res, code, body) {
  res.writeHead(code, { 'content-type': 'application/json' });
  res.end(JSON.stringify(body));
}

function readBody(req) {
  return new Promise((done) => {
    let text = '';
    req.on('data', (part) => { text += part; });
    req.on('end', () => {
      try { done(JSON.parse(text || '{}')); } catch { done({}); }
    });
  });
}

function tokenOf(req) {
  const bearer = (req.headers.authorization || '').replace(/^Bearer /, '');
  return req.headers['x-rl-token'] || bearer;
}

async function loadTitles() {
  try {
    const r = await fetch(`${REAL}/pc-catalog.json`);
    const cat = await r.json();
    const map = new Map();
    for (const ti of cat.titles || []) {
      for (const id of ti.pc || []) map.set(id, ti.id);
    }
    titleOf = map;
    console.log(`Catalog: ${map.size} recomps in ${(cat.titles || []).length} original games`);
  } catch (e) {
    console.log(`Catalog not loaded (${e.message}): no per-game totals`);
  }
}

const MAX_SESSIONS = 500;

// Sessions joined by their [start, secs], newest 500 kept: a resend adds nothing twice.
function joinSessions(had, sent, now) {
  const byKey = new Map();
  for (const x of [...had, ...(Array.isArray(sent) ? sent : [])]) {
    const ok = Array.isArray(x) && Number.isInteger(x[0]) && x[0] > 0 && x[0] <= now + 86400 && Number.isInteger(x[1]) && x[1] > 0 && x[1] <= MAX_SECS;
    if (ok) byKey.set(`${x[0]}|${x[1]}`, [x[0], x[1]]);
  }
  return [...byKey.values()].sort((a, b) => a[0] - b[0]).slice(-MAX_SESSIONS);
}

// One PC's numbers in: the larger of what's stored and what's sent, so a late resend never lowers it.
function takePlay(pc, games) {
  const now = Math.floor(Date.now() / 1000);
  let took = 0;
  for (const g of (games || []).slice(0, 500)) {
    const secs = Number(g && g.secs);
    const last = Number(g && g.last) || 0;
    const bad = typeof g?.game !== 'string' || !g.game || !(secs >= 0) || secs > MAX_SECS || last > now + 86400;
    if (bad) continue;
    const key = `${pc}|${g.game}`;
    const had = rows.get(key) || { pc, game: g.game, secs: 0, last: 0, sessions: [] };
    const sessions = joinSessions(had.sessions, g.sessions, now);
    rows.set(key, { pc, game: g.game, secs: Math.max(had.secs, secs), last: Math.max(had.last, last), sessions });
    took += 1;
  }
  return took;
}

// The account's totals over all its PCs, per game (with every PC's sessions) and per original game.
function totals() {
  const games = {};
  const titles = {};
  const add = (into, key, row) => {
    const t = into[key] || (into[key] = { secs: 0, last: 0 });
    t.secs += row.secs;
    t.last = Math.max(t.last, row.last);
  };
  const now = Math.floor(Date.now() / 1000);
  for (const row of rows.values()) {
    add(games, row.game, row);
    games[row.game].sessions = joinSessions(games[row.game].sessions || [], row.sessions, now);
    // A game inside a recomp with several ("recomp--game") isn't mapped to an original game yet.
    const title = titleOf.get(row.game);
    if (title) add(titles, title, row);
  }
  return { ok: true, games, titles };
}

async function launcherCall(req, res, url) {
  const a = url.searchParams.get('a') || '';
  const token = tokenOf(req);
  // A fake token from before a restart still counts, so restarting (say with --no-play) keeps the
  // launchers signed in. Its rows start empty again, as nothing is kept.
  const pc = pcs.get(token) || (token.startsWith('fake-') ? token : undefined);

  if (a === 'link-start') {
    const body = await readBody(req);
    const poll = `poll-${Math.random().toString(36).slice(2)}`;
    polls.set(poll, String(body.device || 'PC'));
    const origin = `http://${req.headers.host}`;
    return send(res, 200, { code: 'FAKE-0000', poll, url: `${origin}/activate`, interval: 2, expires: 600 });
  }
  if (a === 'link-poll') {
    const body = await readBody(req);
    const device = polls.get(body.poll);
    if (!device) return send(res, 200, { status: 'expired' });
    polls.delete(body.poll);
    const newToken = `fake-${Math.random().toString(36).slice(2)}`;
    pcs.set(newToken, `${device} (${pcs.size + 1})`);
    console.log(`Signed in: ${pcs.get(newToken)}`);
    return send(res, 200, { status: 'ok', token: newToken, account: ACCOUNT });
  }
  if (a === 'play') {
    if (NO_PLAY || !pc) return send(res, 401, { ok: false, error: 'Sign in again.' });
    if (req.method === 'POST') {
      const body = await readBody(req);
      const took = takePlay(pc, body.games);
      console.log(`Playtime from ${pc}: ${took} game(s)`);
      return send(res, 200, { ok: true });
    }
    return send(res, 200, totals());
  }
  if (!pc) return proxy(req, res, url);
  if (a === 'me') return send(res, 200, { ok: true, account: ACCOUNT, saves: { used: 0, quota: 150 * 1024 * 1024 } });
  if (a === 'signout') {
    pcs.delete(token);
    return send(res, 200, { ok: true });
  }
  // Cloud saves, the saved list, the inbox and the rest: nothing there for a fake account.
  return send(res, 200, { ok: true, saved: [], items: [], copies: [], saves: [] });
}

function proxy(req, res, url) {
  const headers = { ...req.headers, host: new URL(REAL).host };
  delete headers['accept-encoding'];
  const out = https.request(`${REAL}${url.pathname}${url.search}`, { method: req.method, headers }, (up) => {
    res.writeHead(up.statusCode || 502, up.headers);
    up.pipe(res);
  });
  out.on('error', (e) => send(res, 502, { ok: false, error: e.message }));
  req.pipe(out);
}

const server = http.createServer(async (req, res) => {
  const url = new URL(req.url, `http://${req.headers.host || 'localhost'}`);
  if (url.pathname === '/activate') {
    res.writeHead(200, { 'content-type': 'text/html' });
    return res.end('<p>Fake sign-in: approved. Go back to the launcher.</p>');
  }
  if (url.pathname === '/state') {
    return send(res, 200, { pcs: [...pcs.values()], rows: [...rows.values()], ...totals() });
  }
  if (url.pathname.endsWith('/api/launcher.php')) return launcherCall(req, res, url);
  return proxy(req, res, url);
});

loadTitles().then(() => {
  server.listen(PORT, '0.0.0.0', () => {
    console.log(`Fake site on port ${PORT}${NO_PLAY ? ' (playtime calls refused, like the real site today)' : ''}`);
  });
});
