'use strict';
// Recomp Achievements: the game page's Achievements tab, with its dev tools. Only for accounts the
// website lets in (test accounts for now): the tab doesn't show for anyone else. The core does the
// channel, the toasts, the local store and the syncing (ach.rs).
(() => {
  const A = (S.ach = S.ach || { allowed: false, games: {}, log: {}, mem: {}, rule: {}, devOpen: false });
  const STAR = (on) => `<svg class="ach-star${on ? ' on' : ''}" viewBox="0 0 64 64" width="40" height="40" aria-hidden="true"><path d="M32 5.5l7.9 16.1 17.7 2.6-12.8 12.5 3 17.6L32 46l-15.8 8.3 3-17.6L6.4 24.2l17.7-2.6z"/></svg>`;
  const onTab = (id) => S.view === 'game' && S.arg === id && S.tab === 'ach';
  const REASONS = {
    earlier: N_('Unlocked before this session'),
    save_loaded: N_('Came back with a loaded save'),
  };
  const why = (r) => {
    if (!r) return '';
    if (REASONS[r]) return t(REASONS[r]);
    if (r.startsWith('casual:')) return tf('The game said: {reason}', { reason: r.slice(7) });
    if (r.startsWith('voids:')) return tf('A setting that voids unlocks was on: {keys}', { keys: r.slice(6) });
    return r;
  };

  async function access(fresh) {
    try { A.allowed = !!(await invoke('ach_access', { fresh: !!fresh })).allowed; } catch (e) { A.allowed = false; }
    if (S.view === 'game') refreshGame();
  }
  async function load(id, fresh) {
    try { A.games[id] = await invoke('ach_game', { id, fresh: !!fresh }); } catch (e) { A.games[id] = { allowed: false }; }
    if (A.games[id] && A.games[id].allowed === false) A.allowed = false;
    if (onTab(id)) refreshGame();
  }
  let logTimer = 0;
  async function loadLog(id) {
    try { A.log[id] = await invoke('ach_log', { id }); } catch (e) { A.log[id] = []; }
    const box = $('#ach-log');
    if (box && onTab(id)) { const end = box.scrollTop + box.clientHeight >= box.scrollHeight - 8; box.innerHTML = logLines(id); if (end) box.scrollTop = box.scrollHeight; }
  }

  window.achTabs = (id) => (A.allowed && (byId(id) || S.library[id]) ? [['ach', t('Achievements'), `<b class="tab-beta">${esc(t('Dev'))}</b>`]] : []);

  function row(a, d) {
    const id = Number(a.id);
    const u = (d.unlocks || []).find((x) => x.id === id);
    const fromSite = !u && (d.unlocked || []).includes(id);
    const got = !!u || fromSite;
    const pr = (d.progress || {})[id];
    const broken = (d.broken || {})[id];
    const pts = Number(a.points) || 0;
    const ranked = u ? (u.ranked != null ? u.ranked : !u.casual) : fromSite;
    const tag = got
      ? `<span class="tag ${ranked ? 'ok' : ''}" title="${esc(u ? why(u.server_reason || u.reason) : '')}">${esc(ranked ? t('Ranked') : t('Casual'))}</span>`
      : `<span class="tag">${esc(t('Locked'))}</span>`;
    const bar = !got && pr && pr.target ? `<div class="ach-bar" role="progressbar" aria-valuemin="0" aria-valuemax="${pr.target}" aria-valuenow="${pr.value}"><i style="width:${Math.min(100, (pr.value / pr.target) * 100)}%"></i></div><span class="ach-pr">${esc(tf('{n} of {total}', { n: fmtN(pr.value), total: fmtN(pr.target) }))}</span>` : '';
    const when = u ? `<span class="ach-when">${esc(date(u.at * 1000))}${u.casual && !u.ranked ? ' · ' + esc(why(u.server_reason || u.reason)) : ''}</span>` : '';
    return `<div class="ach-row${got ? ' got' : ''}">${STAR(got)}<div class="ach-k"><b>${esc(a.title || tf('Achievement {n}', { n: id }))}</b><span>${esc(a.description || '')}</span>${bar}${when}${broken ? `<span class="ach-broken">${esc(tf('This rule can\'t be checked in this version: {why}', { why: broken }))}</span>` : ''}</div><div class="ach-side"><span class="ach-pts">${pts ? esc(tf('{n} pts', { n: pts })) : ''}</span>${tag}</div></div>`;
  }

  function logLines(id) {
    const l = A.log[id] || [];
    if (!l.length) return `<p class="muted" style="margin:0">${esc(t('Nothing yet. Start the game from here: its messages show as they come.'))}</p>`;
    return l.slice(-250).map((x) => `<div class="ach-ln d${x.dir === '<' ? 'in' : x.dir === '>' ? 'out' : x.dir === '!' ? 'bad' : 'note'}"><span>${esc(new Date(x.at * 1000).toLocaleTimeString(curLocale()))}</span><b>${esc(x.dir)}</b><code>${esc(x.line)}</code></div>`).join('');
  }

  function hexDump(base, hex) {
    const b = (hex.match(/../g) || []);
    const rows = [];
    for (let i = 0; i < b.length; i += 8) {
      const chunk = b.slice(i, i + 8);
      const ascii = chunk.map((h) => { const c = parseInt(h, 16); return c >= 32 && c < 127 ? String.fromCharCode(c) : '.'; }).join('');
      rows.push(`<div><span>${(base + i).toString(16).padStart(8, '0')}</span><code>${chunk.join(' ')}</code><i>${esc(ascii)}</i></div>`);
    }
    return rows.join('') || `<p class="muted" style="margin:0">${esc(t('The game sent nothing back for that address.'))}</p>`;
  }

  function ruleResult(r) {
    if (!r) return '';
    if (r.error) return `<p class="ach-bad">${esc(r.error)}</p>`;
    const g = (r.groups || []).map((x, i) => `<div class="ach-grp"><b>${esc(i === 0 ? t('Core') : tf('Alternative {n}', { n: i }))}: ${esc(x.true ? t('true') : t('false'))}</b>${(x.parts || []).map((p) => `<code class="${p.true === true ? 'yes' : p.true === false ? 'no' : ''}">${esc(p.text)}${p.left != null ? ` → ${esc(String(p.left))} vs ${esc(String(p.right))}` : p.value != null ? ` → ${esc(String(p.value))}` : ''}</code>`).join('')}</div>`).join('');
    return `<p class="ach-verdict ${r.true ? 'yes' : 'no'}">${esc(r.true ? t('True right now') : t('Not true right now'))}</p>${g}${(r.notes || []).map((n) => `<p class="muted" style="margin:6px 0 0">${esc(n)}</p>`).join('')}`;
  }

  function devTools(id, d) {
    const m = A.mem[id] || { addr: '0', size: 64 };
    const r = A.rule[id] || { text: '' };
    const live = d.live ? (d.connected ? t('Connected') : t('Running, not connected')) : t('Not running');
    return `<div class="card ach-dev"><div class="card-h"><h3>${esc(t('Developer Tools'))}</h3><span class="tag">${esc(live)}</span></div>
      <p class="muted" style="margin:6px 0 14px">${esc(d.session ? tf('Session {id}', { id: d.session }) : t('A session starts when you press Play.'))}${d.hello ? ' · ' + esc(tf('SDK {sdk}, {console}', { sdk: d.hello.sdk, console: d.hello.console || '—' })) : ''}${d.found ? ' · ' + esc(tf('ReXGlue unlock file: {folder}', { folder: d.found })) : ''}</p>
      <div class="ach-two">
        <div class="ach-tool"><b>${esc(t('Channel Log'))}</b><div class="ach-log" id="ach-log">${logLines(id)}</div></div>
        <div class="ach-tool"><b>${esc(t('Memory Viewer'))}</b>
          <div class="ach-inline"><input class="field" id="ach-addr" value="${esc(m.addr)}" spellcheck="false" aria-label="${esc(t('Address (hex)'))}" placeholder="${esc(t('Address (hex)'))}"><select class="sel" id="ach-size" aria-label="${esc(t('Bytes'))}">${[16, 32, 64, 128, 256].map((n) => `<option${Number(m.size) === n ? ' selected' : ''}>${n}</option>`).join('')}</select><button type="button" class="btn" data-ach="peek" data-id="${esc(id)}"${d.connected ? '' : ' disabled'}>${esc(t('Read'))}</button></div>
          <div class="ach-hex">${m.hex != null ? hexDump(m.base || 0, m.hex) : m.error ? `<p class="ach-bad">${esc(m.error)}</p>` : `<p class="muted" style="margin:0">${esc(t('Reads the running game\'s memory through its channel.'))}</p>`}</div>
        </div>
      </div>
      <div class="ach-tool" style="margin-top:14px"><b>${esc(t('Rule Tester'))}</b>
        <div class="ach-inline"><input class="field ach-mono" id="ach-rule" value="${esc(r.text)}" spellcheck="false" placeholder="0xH0019=2_0xH0010>5" aria-label="${esc(t('Rule'))}"><button type="button" class="btn" data-ach="rule" data-id="${esc(id)}"${d.connected ? '' : ' disabled'}>${esc(t('Test'))}</button></div>
        <div class="ach-rule-out">${ruleResult(r.result)}</div>
      </div>
      <div class="ach-acts"><button type="button" class="btn" data-ach="toast" data-id="${esc(id)}">${esc(t('Show Test Toast'))}</button><button type="button" class="btn" data-ach="toast-m" data-id="${esc(id)}">${esc(t('Show Mastered Toast'))}</button><button type="button" class="btn danger" data-ach="reset" data-id="${esc(id)}">${esc(t('Reset This Game\'s Unlocks'))}</button></div>
    </div>`;
  }

  window.achBody = (a) => {
    const id = a.id;
    const d = A.games[id];
    if (!d) { load(id, true); return `<div class="stack"><div class="card"><p class="muted" style="margin:0">${esc(t('Loading achievements…'))}</p></div></div>`; }
    if (!A.loaded || A.loaded !== id) { A.loaded = id; load(id, true); loadLog(id); }
    const list = d.list && Array.isArray(d.list.achievements) ? d.list.achievements : [];
    // A native list (Xbox 360) can come without titles: its unlocks show by number.
    const shown = list.length ? list : (d.unlocks || []).map((u) => ({ id: u.id, title: '', points: 0 }));
    const got = shown.filter((x) => (d.unlocked || []).includes(Number(x.id))).length;
    // A native list with only a "total" (no names yet) counts against that.
    const of = Math.max(shown.length, Number(d.count) || 0);
    const pct = of ? Math.round((got / of) * 100) : 0;
    const status = d.paused ? tf('Its list is paused for this version: {why}.', { why: d.paused }) : d.live ? (d.connected ? t('The game is connected: unlocks show as they happen.') : t('The game is running. It hasn\'t connected (it may not have achievements built in yet).')) : '';
    const head = `<div class="card ach-head"><div class="ach-sum"><div><span class="ach-big">${esc(tf('{n} of {total}', { n: got, total: of }))}</span><span class="muted">${esc(d.total ? tf('{n} of {total} points', { n: d.points, total: d.total }) : t('Achievements'))}</span></div>${d.mastered ? `<span class="ach-mastered">${esc(t('Mastered'))}</span>` : ''}</div>
      <div class="ach-bar big" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${pct}"><i style="width:${pct}%"></i></div>
      ${status ? `<p class="muted" style="margin:12px 0 0">${esc(status)}</p>` : ''}
      <div class="set-row" style="margin-top:12px"><div class="k"><b>${esc(t('Unlock Sound'))}</b><span>${esc(t('A short chime with each toast over the game.'))}</span></div><button type="button" class="switch" role="switch" aria-checked="${!!d.sound}" aria-label="${esc(t('Unlock Sound'))}" data-ach="sound" data-id="${esc(id)}"></button></div></div>`;
    const body = shown.length
      ? `<div class="card ach-list">${shown.map((x) => row(x, d)).join('')}</div>`
      : `<div class="card"><p style="margin:0">${esc(t('No achievements for this game yet.'))}</p><p class="muted" style="margin:6px 0 0">${esc(t('A list for it shows here once there is one. For testing, put one in the achievements\\dev-lists folder as <game id>.json.'))}</p></div>`;
    return `<div class="stack">${head}${body}${d.dev || (typeof devOn === 'function' && devOn()) ? devTools(id, d) : ''}</div>`;
  };

  document.addEventListener('click', async (e) => {
    const b = e.target.closest && e.target.closest('[data-ach]');
    if (!b) return;
    const id = b.dataset.id, k = b.dataset.ach;
    if (k === 'sound') {
      const on = b.getAttribute('aria-checked') !== 'true';
      b.setAttribute('aria-checked', String(on));
      if (A.games[id]) A.games[id].sound = on;
      invoke('ach_sound', { on }).catch(() => {});
    } else if (k === 'peek') {
      const addrText = ($('#ach-addr') || {}).value || '0';
      const size = Number(($('#ach-size') || {}).value || 64);
      const base = parseInt(addrText.replace(/^0x/i, ''), 16);
      A.mem[id] = { addr: addrText, size };
      if (!Number.isFinite(base)) { A.mem[id].error = t('That address isn\'t a hex number.'); return refreshGame(); }
      try { A.mem[id].hex = await invoke('ach_peek', { id, address: base, size }); A.mem[id].base = base; } catch (err) { A.mem[id].error = String(err && err.message || err); }
      refreshGame();
    } else if (k === 'rule') {
      const text = ($('#ach-rule') || {}).value || '';
      A.rule[id] = { text };
      try { A.rule[id].result = await invoke('ach_rule', { id, rule: text }); } catch (err) { A.rule[id].result = { error: String(err && err.message || err) }; }
      refreshGame();
    } else if (k === 'toast' || k === 'toast-m') {
      invoke('ach_test_toast', { id, mastered: k === 'toast-m' }).catch(() => {});
    } else if (k === 'reset') {
      const name = (byId(id) || S.library[id] || {}).name || id;
      modal(`<h2>${esc(tf('Reset achievements for {name}?', { name }))}</h2><p>${esc(t('Its unlocks on this PC go, so they can be earned again. What the website has stays.'))}</p><div class="acts"><button type="button" class="btn" data-x="no">${esc(t('Keep Them'))}</button><button type="button" class="btn danger" data-x="yes">${esc(t('Reset'))}</button></div>`, async (x, close) => {
        if (x.dataset.x !== 'yes') return;
        close();
        await invoke('ach_reset', { id }).catch(() => {});
        load(id);
      }, t('Reset achievements'));
    }
  });
  // Enter in the dev tools' fields.
  document.addEventListener('keydown', (e) => {
    if (e.key !== 'Enter' || !e.target || !e.target.id) return;
    const btn = e.target.id === 'ach-rule' ? $('[data-ach="rule"]') : e.target.id === 'ach-addr' ? $('[data-ach="peek"]') : null;
    if (btn && !btn.disabled) { e.preventDefault(); btn.click(); }
  });


  // ---------- the Achievements page ----------
  // Testers get their profile, games and the leaderboard. Everyone else gets the alpha page: what
  // it is, and the tester sign-up (or where their application stands, or Sign In).
  const WEB = 'https://recomplauncher.com/achievements';
  const STARP = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 2.8l2.8 5.9 6.4.8-4.7 4.4 1.2 6.4L12 17.2 6.3 20.3l1.2-6.4-4.7-4.4 6.4-.8z"/></svg>';
  const EXT = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M14 4h6v6M20 4l-9 9M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5"/></svg>';
  const TERMS = `Recomp Achievements is in alpha. Things will change, and unlocks or points may be reset while we test.
What we collect while you test:
- Your username, and the achievements you unlock: the game, which achievement, and when.
- Your play sessions: when a game starts and stops, its version, and whether a save was loaded. This is how Ranked tells real play from copied saves.
- The achievement list read from your own copy of a game: names, descriptions and points only. Never your game files.
- Your system (Windows, Linux, Mac or Steam Deck), your RecompLauncher version, and the reports you choose to send us.
We use it only to run and improve Recomp Achievements and its anti-cheat. We don't sell it or share it.
Your username, points, unlocks and Mastered games show on the leaderboards and the achievement pages.
Play fair. Unlocks that look copied or cheated count as Casual, and testers who abuse the system can be removed.
You can stop testing at any time. Ask us on Discord and we'll clear your achievement data.
Your account's Terms and Privacy still apply.`;
  Object.assign(A, { status: null, profile: null, list: null, boards: {}, ptab: 'ov', gfilter: 'all', bgame: '', form: null, onPage: false });
  const redraw = () => { if (S.view === 'achievements') render(true); };
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);

  async function loadStatus(fresh) {
    try {
      const s = await invoke('ach_status', { fresh: !!fresh });
      const changed = !same(s, A.status);
      A.status = s;
      A.allowed = !!s.tester;
      if (changed) { redraw(); if (S.view === 'game') refreshGame(); }
      if (s.tester) { loadProfile(); loadBoard(A.bgame); }
      loadList();
    } catch (e) { /* stays as it was */ }
  }
  async function loadProfile() {
    try { const r = await invoke('ach_profile'); if (r.tester === false) return loadStatus(false); if (!same(r.profile, A.profile)) { A.profile = r.profile || null; redraw(); achSide(); } } catch (e) { /* offline */ }
  }
  async function loadList() {
    try { const r = await invoke('ach_games'); const l = Array.isArray(r.games) ? r.games : []; if (!same(l, A.list)) { A.list = l; redraw(); } } catch (e) { /* offline */ }
  }
  async function loadBoard(game) {
    try { const r = await invoke('ach_board', { game: game || null }); if (Array.isArray(r.board) && !same(r.board, A.boards[game || ''])) { A.boards[game || ''] = r.board; redraw(); } } catch (e) { /* offline */ }
  }

  // A points badge on the sidebar's Achievements, for testers.
  function achSide() {
    const nav = document.querySelector('.side .nav[data-go="achievements"]');
    if (nav) {
      let b = nav.querySelector('.ach-side-n');
      const pts = A.allowed && A.profile ? Number(A.profile.points) || 0 : 0;
      if (pts && !b) { b = document.createElement('span'); b.className = 'ach-side-n'; nav.appendChild(b); }
      if (b) { if (pts) { b.textContent = fmtN(pts); b.title = tf('{n} Ranked points', { n: fmtN(pts) }); } else b.remove(); }
    }
    // Opening the page asks the website again, so an approval switches it over by itself.
    const on = S.view === 'achievements';
    if (on && !A.onPage) loadStatus(true);
    A.onPage = on;
  }
  window.achSide = achSide;

  const user = () => (S.account && S.account.signedIn && S.account.user) || null;
  const cat = (id) => byId(id) || null;
  const coverOf = (g) => { const a = cat(g.id); return (a && a.cover) || g.cover || ''; };
  const nameOf = (g) => { const a = cat(g.id); return (a && a.name) || g.name || g.id; };
  const COVERS = ['a', 'b', 'c', 'd', 'e', 'f', 'g'];
  const cov = (g, cls = '') => { const c = coverOf(g); const k = COVERS[[...String(g.id)].reduce((n, ch) => n + ch.charCodeAt(0), 0) % COVERS.length]; return `<div class="achp-cov ${c ? '' : 'ph-' + k} ${cls}">${c ? pic(thumbOf(c, 320), '', 'loading="lazy"') : `<span>${esc(nameOf(g))}</span>`}</div>`; };
  const bar = (n, of, gold) => `<div class="achp-bar${gold ? ' gold' : ''}" role="progressbar" aria-valuemin="0" aria-valuemax="${of || 0}" aria-valuenow="${n || 0}"><i style="width:${of ? Math.min(100, Math.round((n / of) * 100)) : 0}%"></i></div>`;
  const agoShort = (ts) => {
    if (!ts) return '';
    const s = Math.max(0, Date.now() / 1000 - ts);
    if (s < 3600) return tf('{n}m ago', { n: Math.max(1, Math.floor(s / 60)) });
    if (s < 86400) return tf('{n}h ago', { n: Math.floor(s / 3600) });
    if (s < 86400 * 7) return tf('{n}d ago', { n: Math.floor(s / 86400) });
    return new Date(ts * 1000).toLocaleDateString(curLocale(), { month: 'short', day: 'numeric' });
  };
  const chip = (ranked) => `<span class="achp-chip ${ranked ? 'rk' : 'cs'}">${esc(ranked ? t('Ranked') : t('Casual'))}</span>`;
  const head = (sub, btn) => `<div class="head achp-head"><div><div class="achp-h"><h1>${esc(t('Achievements'))}</h1><span class="achp-pill">${esc(t('Alpha'))}</span></div><p class="achp-sub">${esc(sub)}</p></div>${btn}</div>`;
  const webBtn = (label) => `<button type="button" class="btn" data-act="url" data-url="${WEB}">${EXT}${esc(label)}</button>`;
  // As many columns as there are cards (up to `max`), so a row never has a lone leftover.
  const cols = (n, max) => (n <= max ? n : n % max === 0 ? max : n % (max - 1) === 0 ? max - 1 : max);

  // Every game with a list, with this player's progress on it.
  function games() {
    const mine = new Map(((A.profile && A.profile.games) || []).map((g) => [g.id, g]));
    const out = (A.list || []).map((g) => Object.assign({ unlocked: 0, points: 0, points_total: g.points, achievements: g.achievements }, g, mine.get(g.id) || {}));
    for (const g of mine.values()) if (!out.some((x) => x.id === g.id)) out.push(g);
    return out;
  }
  const isPlaying = (g) => g.unlocked > 0 && !g.mastered;

  function pageTester() {
    const p = A.profile || {};
    const u = user() || {};
    const recent = p.recent || [];
    const last = recent[0];
    const line = [p.rank ? tf('Rank #{rank} of {players} players', { rank: fmtN(p.rank), players: fmtN(p.players || 0) }) : t('No Ranked points yet'), last ? tf('Last unlock: {title}, {when}', { title: last.title, when: agoShort(last.at) }) : ''].filter(Boolean).join(' · ');
    const stat = (v, k, small) => `<div class="achp-stat"><b>${v}${small ? `<small>${esc(small)}</small>` : ''}</b><span>${esc(k)}</span></div>`;
    const prof = `<section class="card achp-prof">${typeof avatar === 'function' ? avatar(u, 64) : ''}<div class="achp-prof-n"><h2>${esc(u.username || t('Tester'))} <span class="achp-pill c">${esc(t('Tester'))}</span></h2><span>${esc(line)}</span></div>
      <div class="achp-stats">${stat(fmtN(p.points || 0), t('Ranked Points'))}${stat(p.rank ? '#' + fmtN(p.rank) : '—', t('Rank'), p.rank ? tf('of {n}', { n: fmtN(p.players || 0) }) : '')}${stat(fmtN(p.unlocks || 0), t('Achievements'))}${stat(fmtN(p.mastered_count || 0), t('Mastered Games'))}</div></section>`;
    const tabs = [['ov', t('Overview')], ['games', t('Games')], ['lb', t('Leaderboard')]];
    const tabBar = `<nav class="achp-tabs" role="tablist">${tabs.map(([k, l]) => `<button type="button" role="tab" aria-selected="${A.ptab === k}" class="${A.ptab === k ? 'on' : ''}" data-ach="ptab" data-k="${k}">${esc(l)}</button>`).join('')}</nav>`;
    const body = A.ptab === 'games' ? paneGames() : A.ptab === 'lb' ? paneBoard() : paneOverview();
    return `<div class="page achp">${head(t('Your Recomp Achievements across every game. Thanks for testing: report anything odd on Discord.'), webBtn(t('Leaderboards on the Web')))}${prof}${tabBar}${body}</div>`;
  }

  function progCard(g) {
    return `<button type="button" class="card achp-prog" data-ach="open-game" data-id="${esc(g.id)}">${cov(g)}<div><strong>${esc(nameOf(g))}</strong>${bar(g.unlocked, g.achievements, g.mastered)}<div class="achp-row"><span>${esc(tf('{n} of {total}', { n: fmtN(g.unlocked || 0), total: fmtN(g.achievements || 0) }))}</span><span>${esc(tf('{n} / {total} pts', { n: fmtN(g.points || 0), total: fmtN(g.points_total || 0) }))}</span></div></div></button>`;
  }

  function paneOverview() {
    const p = A.profile || {};
    const playing = ((p.games) || []).filter((g) => g.last).sort((a, b) => (b.last || 0) - (a.last || 0)).slice(0, 3);
    const keep = playing.length
      ? `<div class="achp-grid" style="--c:${playing.length}">${playing.map(progCard).join('')}</div>`
      : `<div class="card achp-empty">${STARP}<div><strong>${esc(t('Nothing Started Yet'))}</strong><span>${esc(t('Start a game with achievements and your progress shows here.'))}</span></div><button type="button" class="btn sm" data-ach="ptab" data-k="games">${esc(t('See the Games'))}</button></div>`;
    const recent = (p.recent || []).slice(0, 8);
    const rec = recent.length
      ? `<div class="card achp-list">${recent.map((r) => `<button type="button" class="achp-un" data-ach="open-game" data-id="${esc(r.game)}"><span class="achp-star">${STARP}</span><div><strong>${esc(r.title || tf('Achievement {n}', { n: r.id }))}</strong><small>${esc([r.name || nameOf({ id: r.game }), r.description].filter(Boolean).join(' · '))}</small></div><span class="achp-pts">${esc(tf('{n} pts', { n: fmtN(r.points || 0) }))}</span>${chip(r.ranked)}<span class="achp-when">${esc(agoShort(r.at))}</span></button>`).join('')}</div>`
      : `<div class="card achp-empty">${STARP}<div><strong>${esc(t('No Unlocks Yet'))}</strong><span>${esc(t('Your unlocks show here the moment you earn them.'))}</span></div></div>`;
    const ms = p.mastered || [];
    const shortest = games().filter((g) => g.achievements > 0).sort((a, b) => a.achievements - b.achievements)[0];
    const mast = ms.length
      ? `<div class="achp-grid" style="--c:${ms.length % 2 === 0 ? 2 : 1}">${ms.map((m) => `<button type="button" class="card achp-mcard" data-ach="open-game" data-id="${esc(m.id)}">${cov(m, 'wide')}<span class="achp-mbadge">${STARP}${esc(t('Mastered'))}</span><strong>${esc(nameOf(m))}</strong><small>${esc([date(m.at * 1000), m.ranked ? t('Ranked') : t('Casual')].join(' · '))}</small></button>`).join('')}</div>`
      : `<div class="card achp-empty gold">${STARP}<div><strong>${esc(t('No Mastered Games Yet'))}</strong><span>${esc(shortest ? tf('Unlock every achievement in a game and its Mastered badge lands here. {name} is the shortest list ({n}).', { name: nameOf(shortest), n: shortest.achievements }) : t('Unlock every achievement in a game and its Mastered badge lands here.'))}</span></div></div>`;
    const sec = (h, inner, link) => `<section class="achp-sec"><div class="achp-sec-h"><h3>${esc(h)}</h3>${link || ''}</div>${inner}</section>`;
    return `<div class="achp-pane">${sec(t('Keep Going'), keep, `<button type="button" class="achp-link" data-ach="ptab" data-k="games">${esc(t('All Games'))}</button>`)}${sec(t('Recent Unlocks'), rec)}${sec(t('Mastered'), mast)}${askCard()}</div>`;
  }
  const askCard = () => `<section class="card achp-ask"><div><strong>${esc(t('Don\'t See a Game You Want?'))}</strong><span>${esc(t('Ask its developer to add Recomp Achievements. It\'s free, and most developers are happy to hear a game is wanted.'))}</span></div><button type="button" class="btn sm" data-act="url" data-url="${WEB}">${esc(t('How Developers Add It'))}</button></section>`;

  function paneGames() {
    const all = games();
    const f = { all: () => true, playing: isPlaying, mastered: (g) => !!g.mastered, none: (g) => !g.unlocked };
    const counts = Object.fromEntries(Object.keys(f).map((k) => [k, all.filter(f[k]).length]));
    const labels = { all: t('All'), playing: t('Playing'), mastered: t('Mastered'), none: t('Not Started') };
    const list = all.filter(f[A.gfilter] || f.all).sort((a, b) => (b.last || 0) - (a.last || 0) || nameOf(a).localeCompare(nameOf(b)));
    const card = (g) => {
      const inst = !!S.library[g.id], inCat = !!cat(g.id);
      const btn = inst ? `<button type="button" class="btn sm main" data-act="play" data-id="${esc(g.id)}">${esc(t('Play'))}</button>` : inCat ? `<button type="button" class="btn sm" data-act="install" data-id="${esc(g.id)}">${esc(t('Install'))}</button>` : '';
      const info = g.mastered ? tf('{n} of {total} · {kind}', { n: fmtN(g.unlocked), total: fmtN(g.achievements), kind: g.mastered === 'ranked' ? t('Ranked') : t('Casual') }) : tf('{n} of {total} · {pts} pts', { n: fmtN(g.unlocked || 0), total: fmtN(g.achievements || 0), pts: fmtN(g.points || 0) });
      return `<div class="achp-gcard" data-ach="open-game" data-id="${esc(g.id)}" role="button" tabindex="0"><div class="achp-cwrap">${cov(g)}${inst ? `<span class="achp-pill c achp-tagm">${esc(t('Installed'))}</span>` : ''}${g.mastered ? `<span class="achp-mbadge">${STARP}${esc(t('Mastered'))}</span>` : ''}</div><strong>${esc(nameOf(g))}</strong>${bar(g.unlocked, g.achievements, g.mastered)}<div class="achp-row"><span>${esc(info)}</span>${btn}</div></div>`;
    };
    return `<div class="achp-pane"><div class="achp-top"><div class="chips">${Object.keys(f).map((k) => `<button type="button" class="chip${A.gfilter === k ? ' on' : ''}" data-ach="gfilter" data-k="${k}">${esc(labels[k])} ${fmtN(counts[k])}</button>`).join('')}</div></div>
      ${list.length ? `<div class="achp-games">${list.map(card).join('')}</div>` : `<div class="card achp-empty">${STARP}<div><strong>${esc(t('Nothing Here'))}</strong><span>${esc(t('No games match this filter yet.'))}</span></div></div>`}</div>`;
  }

  function paneBoard() {
    const rows = A.boards[A.bgame] || null;
    const me = (user() || {}).username;
    const opts = [['', t('All Games')], ...games().map((g) => [g.id, nameOf(g)])];
    const picker = `<select class="sel" data-ach="bgame" aria-label="${esc(t('Game'))}">${opts.map(([v, l]) => `<option value="${esc(v)}"${v === A.bgame ? ' selected' : ''}>${esc(l)}</option>`).join('')}</select>`;
    const row = (r, you) => `<div class="achp-lr${you ? ' you' : ''}"><span class="achp-rk">${esc(String(r.rank || '—'))}</span>${typeof avatar === 'function' ? avatar({ username: r.username, avatar: r.avatar }, 30) : ''}<strong>${esc(r.username || '')}${you ? ` <em>· ${esc(t('You'))}</em>` : ''}</strong><span class="achp-num">${fmtN(r.points || 0)}<small>${esc(t('Ranked'))}</small></span><span class="achp-num">${fmtN(r.unlocks || 0)}</span><span class="achp-num">${fmtN(r.mastered || 0)}</span></div>`;
    let body;
    if (!rows) body = `<p class="muted" style="margin:12px">${esc(t('Loading the leaderboard…'))}</p>`;
    else if (!rows.length) body = `<p class="muted" style="margin:12px">${esc(t('Nobody has Ranked points here yet.'))}</p>`;
    else {
      body = rows.map((r) => row(r, me && r.username === me)).join('');
      const p = A.profile || {};
      if (me && !A.bgame && !rows.some((r) => r.username === me) && p.rank) body += `<div class="achp-lr gap"><span>⋯</span></div>` + row({ rank: p.rank, username: me, avatar: (user() || {}).avatar, points: p.points, unlocks: p.unlocks, mastered: p.mastered_count }, true);
    }
    return `<div class="achp-pane"><div class="achp-top"><span class="muted">${esc(t('Only Ranked points count.'))}</span>${picker}</div>
      <div class="card achp-lb"><div class="achp-lr hd"><span>#</span><span></span><span>${esc(t('Player'))}</span><span class="achp-num">${esc(t('Points'))}</span><span class="achp-num">${esc(t('Unlocks'))}</span><span class="achp-num">${esc(t('Mastered'))}</span></div>${body}</div></div>`;
  }

  // ---------- the alpha page ----------
  function pageAlpha() {
    const s = A.status || {};
    const signed = !!(S.account && S.account.signedIn);
    const app = s.application || null;
    const pending = signed && app && app.status === 'pending';
    const state = !signed ? 'out' : pending ? 'pending' : s.needs_name ? 'name' : s.apply_open === false ? 'closed' : 'apply';
    const cta = state === 'out' ? `<button type="button" class="btn main" data-act="acc-btn">${esc(t('Sign In to Apply'))}</button>`
      : state === 'pending' ? `<span class="achp-pill">${esc(t('Applied · You\'re on the List'))}</span>`
      : state === 'apply' ? `<button type="button" class="btn main" data-ach="to-apply">${esc(t('Apply to Be a Tester'))}</button>` : '';
    const hero = `<section class="card achp-hero"><div><span class="achp-pill ghost">${esc(t('Now Testing'))}</span><h2>${esc(t('Recomp Achievements Is Here, in Alpha'))}</h2>
      <p>${esc(t('We\'ve started rolling out Recomp Achievements to testers. Achievements pop up the moment you earn them, every game you finish becomes Mastered, and your points put you on the leaderboards. Testers get it first and help shape it before it opens to everyone.'))}</p>
      <div class="achp-cta">${cta}<button type="button" class="btn" data-act="url" data-url="${WEB}">${esc(t('Learn More'))}</button></div></div>
      <div class="achp-demo" aria-hidden="true"><div class="achp-toast"><i>${STARP}</i><div><b>${esc(t('Achievement Unlocked'))}</b><strong>Taste of Power</strong><small>${esc(tf('{n} points', { n: 10 }))}</small></div></div></div></section>`;
    const feats = [
      [t('Unlocks While You Play'), t('Achievements pop up in the corner the moment you earn them, right over your recomp.'), '<path d="M12 3l2.5 5.2 5.7.8-4.1 4 1 5.6L12 16l-5.1 2.6 1-5.6-4.1-4 5.7-.8z"/>'],
      [t('Your Profile and Rating'), t('Every achievement adds to your points and rank, on every PC you play on.'), '<circle cx="12" cy="8.5" r="3.8"/><path d="M4.5 20a7.5 7.5 0 0 1 15 0"/>'],
      [t('Leaderboards'), t('See how you stack up against other players, overall and game by game. Only Ranked unlocks count.'), '<path d="M6 20V11M12 20V4M18 20v-7"/>'],
    ];
    const featHtml = `<div class="achp-grid" style="--c:3">${feats.map(([h, d, ic]) => `<div class="card achp-feat"><i><svg viewBox="0 0 24 24">${ic}</svg></i><strong>${esc(h)}</strong><span>${esc(d)}</span></div>`).join('')}</div>`;
    const wait = (ic, h, p, btn, cls = '') => `<section class="card achp-wait ${cls}"><span class="achp-ring">${ic}</span><div><h3>${esc(h)}</h3><p>${esc(p)}</p></div>${btn}</section>`;
    const CLOCK = '<svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/></svg>';
    const PERSON = '<svg viewBox="0 0 24 24"><circle cx="12" cy="8.5" r="3.8"/><path d="M4.5 20a7.5 7.5 0 0 1 15 0"/></svg>';
    const block = state === 'out' ? wait(PERSON, t('Sign In to Apply'), t('Achievements are tied to your RecompLauncher account, so they follow you to every PC you play on.'), `<button type="button" class="btn main" data-act="acc-btn">${esc(t('Sign In'))}</button>`, 'cy')
      : state === 'pending' ? wait(CLOCK, t('You\'re on the List'), (app.applied ? tf('Applied {date}.', { date: date(Number(app.applied) * 1000 || app.applied) }) + ' ' : '') + t('We approve testers in waves, and this page unlocks on its own once you\'re in. No need to apply again.'), `<button type="button" class="btn" data-act="url" data-url="${esc(discordUrl())}">${esc(t('Join the Discord'))}</button>`)
      : state === 'name' ? wait(PERSON, t('Pick a Username First'), t('Testers show on the leaderboards by their username. Pick one on your account page, then come back here to apply.'), `<button type="button" class="btn main" data-act="acc-btn">${esc(t('Go to Your Account'))}</button>`, 'cy')
      : state === 'closed' ? wait(CLOCK, t('Sign-Ups Are Closed'), t('Tester sign-ups are closed right now. Check back soon.'), `<button type="button" class="btn" data-act="url" data-url="${esc(discordUrl())}">${esc(t('Join the Discord'))}</button>`)
      : applyForm(app && app.status === 'declined');
    const list = (A.list || []).slice(0, 3);
    const mini = (g) => `<div class="achp-mini">${cov(g)}<strong>${esc(nameOf(g))}</strong><small>${esc(tf('{n} achievements · {pts} pts', { n: fmtN(g.achievements || 0), pts: fmtN(g.points || 0) }))}</small></div>`;
    const gamesSec = list.length ? `<section class="achp-sec"><div class="achp-sec-h"><h3>${esc(t('Games with Achievements'))}</h3><span class="muted">${esc(t('More as developers add it'))}</span></div>
      <div class="achp-grid" style="--c:${list.length + 1}">${list.map(mini).join('')}<div class="achp-mini"><div class="achp-cov dash"><span>${esc(t('Your Game?'))}</span></div><strong>${esc(t('Don\'t See a Game?'))}</strong><small>${esc(t('Ask its developer to add it. It\'s free.'))}</small></div></div></section>` : '';
    return `<div class="page achp">${head(t('Earn achievements in your recomps, track them on your profile and climb the leaderboards.'), webBtn(t('Learn More')))}${hero}${featHtml}${block}${gamesSec}</div>`;
  }

  function applyForm(declined) {
    const s = A.status || {};
    if (!A.form) {
      A.form = { os: s.os || 'windows', discord: '', picks: new Set(), other: '', why: '', agree: false, terms: false, busy: false, error: '' };
    }
    const f = A.form;
    const mine = Object.values(S.library || {}).filter((g) => g && g.name && g.tag !== 'own').map((g) => g.name).sort((a, b) => a.localeCompare(b)).slice(0, 24);
    const osOpts = [['windows', 'Windows'], ['linux', 'Linux'], ['mac', 'Mac'], ['steamdeck', 'Steam Deck']];
    const step = (n, h, p) => `<li><b>${n}</b><div><strong>${esc(h)}</strong><span>${esc(p)}</span></div></li>`;
    return `<section class="card achp-apply" id="achp-apply"><div><h3>${esc(t('Become a Tester'))}</h3><p class="achp-lead">${esc(declined ? t('Your last application wasn\'t approved this time. You\'re welcome to apply again.') : t('It takes a minute. We approve testers in waves, and the page unlocks on its own once you\'re in.'))}</p>
      <ol class="achp-steps">${step(1, t('Apply Here'), t('Tell us what you play and accept the tester terms.'))}${step(2, t('Get Approved'), t('No email to wait for. This page switches over by itself.'))}${step(3, t('Play'), t('Start any game from the list and earn achievements as you go.'))}</ol></div>
      <div class="achp-form">
        <label>${esc(t('You Play On'))}<select class="sel" data-af="os">${osOpts.map(([v, l]) => `<option value="${v}"${f.os === v ? ' selected' : ''}>${esc(l)}</option>`).join('')}</select></label>
        <label>${esc(t('Discord Username (Optional)'))}<input class="field" data-af="discord" maxlength="60" value="${esc(f.discord)}" placeholder="${esc(t('So we can reach you about bugs'))}"></label>
        <div class="full achp-lbl">${esc(t('Recomps You Play'))}<div class="achp-picks">${mine.map((n) => `<button type="button" class="${f.picks.has(n) ? 'on' : ''}" data-ach="pick" data-n="${esc(n)}" aria-pressed="${f.picks.has(n)}">${esc(n)}</button>`).join('')}</div><input class="field" data-af="other" maxlength="300" value="${esc(f.other)}" placeholder="${esc(mine.length ? t('Others, separated by commas') : t('The recomps you play, separated by commas'))}"></div>
        <label class="full">${esc(t('Why You Want to Test (Optional)'))}<textarea class="field" data-af="why" maxlength="600" rows="3" placeholder="${esc(t('Anything you\'d like us to know'))}">${esc(f.why)}</textarea></label>
        <details class="full achp-terms"${f.terms ? ' open' : ''}><summary>${esc(t('Tester Terms'))}</summary><div>${TERMS.split('\n').map((l) => `<p>${esc(l)}</p>`).join('')}</div></details>
        <label class="full achp-check"><input type="checkbox" data-af="agree"${f.agree ? ' checked' : ''}><span>${esc(t('I\'ve read the tester terms and agree to them.'))}</span></label>
        <div class="full achp-send"><button type="button" class="btn main" data-ach="apply"${f.agree && !f.busy ? '' : ' disabled'}>${esc(f.busy ? t('Sending…') : t('Apply'))}</button>${f.error ? `<span class="achp-err" role="alert">${esc(f.error)}</span>` : ''}</div>
      </div></section>`;
  }

  window.achPage = () => {
    if (!A.onPage) { A.onPage = true; loadStatus(true); }
    if (!A.status && !(S.account && S.account.signedIn)) A.status = { signed_in: false };
    if (A.status && A.status.tester && A.allowed) return pageTester();
    return pageAlpha();
  };

  function plays() {
    const f = A.form;
    return [...f.picks, ...String(f.other || '').split(',').map((x) => x.trim()).filter(Boolean)].join(', ').slice(0, 300);
  }
  document.addEventListener('input', (e) => {
    const k = e.target && e.target.dataset && e.target.dataset.af;
    if (!k || !A.form) return;
    if (k === 'agree') { A.form.agree = e.target.checked; const b = $('[data-ach="apply"]'); if (b) b.disabled = !A.form.agree || A.form.busy; return; }
    A.form[k] = e.target.value;
  });
  document.addEventListener('toggle', (e) => { if (e.target && e.target.classList && e.target.classList.contains('achp-terms') && A.form) A.form.terms = e.target.open; }, true);
  document.addEventListener('change', (e) => {
    const sel = e.target.closest && e.target.closest('[data-ach="bgame"]');
    if (sel) { A.bgame = sel.value; redraw(); loadBoard(A.bgame); }
  });
  document.addEventListener('click', async (e) => {
    const b = e.target.closest && e.target.closest('[data-ach]');
    if (!b || !b.closest('.achp')) return;
    if (e.target.closest('[data-act]') && b.dataset.ach === 'open-game') return;
    const k = b.dataset.ach;
    if (k === 'ptab') { A.ptab = b.dataset.k; redraw(); if (A.ptab === 'lb') loadBoard(A.bgame); }
    else if (k === 'gfilter') { A.gfilter = b.dataset.k; redraw(); }
    else if (k === 'open-game') { const id = b.dataset.id; if (!id || !(byId(id) || S.library[id])) return; S.nextTab = A.allowed ? 'ach' : 'overview'; go('game', id); }
    else if (k === 'to-apply') { const f = $('#achp-apply'); if (f) f.scrollIntoView({ behavior: 'smooth', block: 'start' }); }
    else if (k === 'pick') { const n = b.dataset.n; if (A.form.picks.has(n)) A.form.picks.delete(n); else A.form.picks.add(n); b.classList.toggle('on'); b.setAttribute('aria-pressed', String(A.form.picks.has(n))); }
    else if (k === 'apply') {
      const f = A.form;
      if (!f || !f.agree || f.busy) return;
      if (!plays()) { f.error = t('Tell us at least one recomp you play.'); return redraw(); }
      f.busy = true; f.error = ''; redraw();
      try {
        const r = await invoke('ach_apply', { form: { agree: true, os: f.os, plays: plays(), why: f.why, discord: f.discord } });
        f.busy = false;
        if (r && r.error) { f.error = String(r.error); return redraw(); }
        A.status = r; A.allowed = !!r.tester; A.form = null; redraw();
      } catch (err) { f.busy = false; f.error = String((err && err.message) || err || t('Something went wrong. Try again in a bit.')); redraw(); }
    }
  });
  document.addEventListener('keydown', (e) => {
    if ((e.key === 'Enter' || e.key === ' ') && e.target && e.target.classList && e.target.classList.contains('achp-gcard')) { e.preventDefault(); e.target.click(); }
  });
  T.event.listen('ach-profile', () => { if (A.allowed) loadProfile(); });
  setTimeout(() => loadStatus(true), 1500);

  T.event.listen('ach-access', ({ payload }) => { A.allowed = !!(payload && payload.allowed); if (S.view === 'game') refreshGame(); loadStatus(false); });
  T.event.listen('ach-changed', ({ payload }) => { const id = payload && payload.game; if (id && (A.games[id] || onTab(id))) load(id); });
  T.event.listen('ach-log', ({ payload }) => { const id = payload && payload.game; if (!id || !onTab(id)) return; clearTimeout(logTimer); logTimer = setTimeout(() => loadLog(id), 250); });
  T.event.listen('account', () => setTimeout(() => { A.profile = null; A.form = null; loadStatus(true); }, 800));
  T.event.listen('game', ({ payload }) => { const id = payload && payload.id; if (id && A.allowed) setTimeout(() => load(id), payload.state === 'running' ? 1500 : 300); });
})();
