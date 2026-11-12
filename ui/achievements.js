'use strict';
// Recomp Achievements: the game page's Achievements tab, with its dev tools. Only for accounts the
// website lets in (test accounts for now): the tab doesn't show for anyone else. The core does the
// channel, the toasts, the local store and the syncing (ach.rs).
// A list's or a game's console by name ("dc" is Dreamcast, "x360" Xbox 360).
function achConsole(c) {
  const hit = (typeof MORE_CONSOLES !== 'undefined' ? MORE_CONSOLES : []).find(([k]) => k === c);
  return hit ? hit[1] : c || '—';
}
(() => {
  const A = (S.ach = S.ach || { allowed: false, games: {}, log: {}, mem: {}, rule: {}, devOpen: false });
  A.discMsg = A.discMsg || {};
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
    if (window.tvRefresh) window.tvRefresh();
  }
  let logTimer = 0;
  async function loadLog(id) {
    try { A.log[id] = await invoke('ach_log', { id }); } catch (e) { A.log[id] = []; }
    const box = $('#ach-log');
    if (box && onTab(id)) { const end = box.scrollTop + box.clientHeight >= box.scrollHeight - 8; box.innerHTML = logLines(id); if (end) box.scrollTop = box.scrollHeight; }
  }

  window.achTabs = (id) => (A.allowed && (byId(id) || S.library[id]) ? [['ach', t('Achievements'), `<b class="tab-beta">${esc(t('Dev'))}</b>`]] : []);

  const WEB_A = 'https://recomplauncher.com/achievements';
  const TYPES = { win: N_('Win'), progression: N_('Progression'), missable: N_('Missable') };
  const WARN = '<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M10.3 3.9L1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0zM12 9v4M12 17h.01"/></svg>';
  const FLAG = '<svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 22V4M4 4h13l-2 4 2 4H4"/></svg>';
  const KINDS = [['didnt_unlock', N_('Didn\'t unlock')], ['too_early', N_('Unlocked by itself or too early')], ['wrong_text', N_('Wrong name or description')], ['other', N_('Something else')]];
  // How many players have it: "12% of players", or "Nobody yet". rate is 0 to 1.
  const rarityText = (rate) => {
    if (rate == null || rate === '') return '';
    // A fraction (0.125), or a percentage already (12.5) from older answers.
    const p = Number(rate) > 1 ? Number(rate) : Number(rate) * 100;
    if (!(p > 0)) return t('Nobody yet');
    return tf('{pct}% of players', { pct: p < 0.1 ? '<0.1' : p < 1 ? p.toFixed(1) : String(Math.round(p)) });
  };
  // Leaderboard values read the same everywhere (the site and the core use the same rules).
  function fmtValue(v, format) {
    v = Math.trunc(Number(v) || 0);
    const neg = v < 0 ? '-' : '', a = Math.abs(v), p2 = (n) => String(n).padStart(2, '0');
    const clock = (secs, tail) => { const h = Math.floor(secs / 3600), m = Math.floor(secs / 60) % 60, x = secs % 60; return h > 0 ? `${neg}${h}:${p2(m)}:${p2(x)}${tail}` : `${neg}${m}:${p2(x)}${tail}`; };
    switch (format) {
      case 'frames': case 'time': return clock(Math.floor(a / 60), '.' + p2(Math.floor((a % 60) * 100 / 60)));
      case 'millisecs': return clock(Math.floor(a / 1000), '.' + String(a % 1000).padStart(3, '0'));
      case 'seconds': return clock(a, '');
      case 'minutes': return `${neg}${Math.floor(a / 60)}:${p2(a % 60)}`;
      default: return fmtNum(v);
    }
  }
  const isBonus = (a) => a && (a.bonus === true || a.bonus === 1 || a.bonus === 'true');
  A.web = A.web || {};
  A.reportedNow = A.reportedNow || {};
  async function loadWeb(game) {
    try { const v = await invoke('ach_game_web', { game }); if (!same(v, A.web[game])) { A.web[game] = v || {}; if (onTab(game)) refreshGame(); redraw(); } } catch (e) { A.web[game] = A.web[game] || {}; }
  }
  // A board's top players, whatever the website calls the list.
  const topOf = (b) => (b && (b.top || b.entries || b.rows || b.players)) || [];
  const nameIn = (r) => r.username || r.user || r.name || '';
  const profileUrl = (name) => `${WEB_A}/u/${encodeURIComponent(name)}`;

  // One leaderboard: its title, the player's best and the top 10.
  function lbCard(game, b, mineBest) {
    const web = ((A.web[game] || {}).leaderboards || []).find((x) => String(x.id) === String(b.id)) || {};
    const fmt = b.format || web.format || 'score';
    const me = (S.account && S.account.user && S.account.user.username) || '';
    const top = topOf(web).slice(0, 10);
    const myRow = me ? top.find((r) => nameIn(r) === me) : null;
    if (!mineBest && myRow) mineBest = { value: myRow.value };
    const best = mineBest ? `<b>${esc(fmtValue(mineBest.value, fmt))}</b>${myRow && myRow.rank ? `<em>#${esc(String(myRow.rank))}</em>` : ''}${mineBest.ranked === false ? `<small>${esc(t('Casual'))}</small>` : ''}` : `<span class="muted">${esc(t('No entry yet'))}</span>`;
    const rows = top.length
      ? `<ol class="ach-lb-top">${top.map((r, i) => `<li class="${nameIn(r) === me ? 'you' : ''}"><span>${esc(String(r.rank || i + 1))}</span><button type="button" class="ach-lb-name" data-act="url" data-url="${esc(profileUrl(nameIn(r)))}">${esc(nameIn(r))}</button><b>${esc(r.display || fmtValue(r.value, fmt))}</b></li>`).join('')}</ol>`
      : `<p class="muted ach-lb-none">${esc(A.web[game] ? t('Nobody has an entry yet.') : t('Loading…'))}</p>`;
    return `<div class="card ach-lb"><div class="ach-lb-h"><b>${esc(b.title || web.title || t('Leaderboard'))}</b>${b.description || web.description ? `<span>${esc(b.description || web.description)}</span>` : ''}</div><div class="ach-lb-me"><span>${esc(t('Your Best'))}</span>${best}</div>${rows}</div>`;
  }
  // Even rows: two per row, a last odd one full width (the CSS does that part).
  const lbGrid = (cards) => `<div class="ach-lbs">${cards.join('')}</div>`;

  function row(a, d) {
    const id = Number(a.id);
    const mine = d.mine || {};
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
    const type = TYPES[String(a.type || '').toLowerCase()] ? String(a.type).toLowerCase() : '';
    const typeTag = type ? `<span class="ach-type t-${type}">${type === 'missable' ? WARN : ''}${esc(t(TYPES[type]))}</span>` : '';
    // Only with a rate for it: no entry means not counted yet, not "nobody has it".
    const rate = ((mine.rarity || {})[id] || {}).rate;
    const rare = rate != null ? rarityText(rate) : '';
    const rep = A.reportedNow[`${d.id}:${id}`] || (mine.reported || {})[id];
    const repTag = rep === 'open' ? `<span class="ach-rep">${esc(t('Reported'))}</span>` : rep === 'fixed' ? `<span class="ach-rep fixed">${esc(t('Fixed'))}</span>` : '';
    const bar = !got && pr && pr.target ? `<div class="ach-bar" role="progressbar" aria-valuemin="0" aria-valuemax="${pr.target}" aria-valuenow="${pr.value}"><i style="width:${Math.min(100, (pr.value / pr.target) * 100)}%"></i></div><span class="ach-pr">${esc(tf('{n} of {total}', { n: fmtN(pr.value), total: fmtN(pr.target) }))}</span>` : '';
    const when = u ? `<span class="ach-when">${esc(date(u.at * 1000))}${u.casual && !u.ranked ? ' · ' + esc(why(u.server_reason || u.reason)) : ''}</span>` : '';
    const miss = type === 'missable' && !got ? `<span class="ach-miss">${WARN}${esc(t('Can be missed. Read the description before playing on.'))}</span>` : '';
    const meta = [rare ? `<span class="ach-rare">${esc(rare)}</span>` : '', repTag].filter(Boolean).join('');
    return `<div class="ach-row${got ? ' got' : ''}">${STAR(got)}<div class="ach-k"><div class="ach-t"><b>${esc(a.title || tf('Achievement {n}', { n: id }))}</b>${typeTag}</div><span>${esc(a.description || '')}</span>${miss}${bar}${when}${broken ? `<span class="ach-broken">${esc(tf('This rule can\'t be checked in this version: {why}', { why: broken }))}</span>` : ''}${meta ? `<div class="ach-meta">${meta}</div>` : ''}</div><div class="ach-side"><span class="ach-pts">${pts ? esc(tf('{n} pts', { n: pts })) : ''}</span>${tag}<button type="button" class="ach-flag" data-ach="report" data-id="${esc(d.id)}" data-aid="${id}" title="${esc(t('Report a Problem'))}" aria-label="${esc(t('Report a Problem'))}">${FLAG}</button></div></div>`;
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
      <p class="muted" style="margin:6px 0 14px">${esc(d.session ? tf('Session {id}', { id: d.session }) : t('A session starts when you press Play.'))}${d.hello ? ' · ' + esc(tf('SDK {sdk}, {console}', { sdk: d.hello.sdk, console: achConsole(d.hello.console) })) : ''}${d.found ? ' · ' + esc(tf('ReXGlue unlock file: {folder}', { folder: d.found })) : ''}</p>
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
    d.id = id;
    if (!A.loaded || A.loaded !== id) { A.loaded = id; load(id, true); loadLog(id); }
    const mine = d.mine || {};
    const list = d.list && Array.isArray(d.list.achievements) ? d.list.achievements : [];
    const boards = d.list && Array.isArray(d.list.leaderboards) ? d.list.leaderboards : [];
    if (boards.length && A.web[id] === undefined) { A.web[id] = null; loadWeb(id); }
    // A native list (Xbox 360) can come without titles: its unlocks show by number.
    const shown = list.length ? list : (d.unlocks || []).map((u) => ({ id: u.id, title: '', points: 0 }));
    const main = shown.filter((x) => !isBonus(x)), bonus = shown.filter(isBonus);
    const has = (x) => (d.unlocked || []).includes(Number(x.id));
    const got = main.filter(has).length;
    // A native list with only a "total" (no names yet) counts against that.
    const of = list.length ? main.length : Math.max(shown.length, Number(d.count) || 0);
    const pct = of ? Math.round((got / of) * 100) : 0;
    const mastered = !!(d.mastered || mine.mastered), beaten = !mastered && !!mine.beaten;
    // A ReXGlue game is watched through its own unlock file, not the library's channel.
    const live = d.rex === true ? t('Watching the game\'s unlock file: unlocks show as the game saves them.') : d.rex === false ? t('Waiting for the game\'s first unlock.') : d.connected ? t('The game is connected: unlocks show as they happen.') : t('The game is running. It hasn\'t connected (it may not have achievements built in yet).');
    const status = d.paused ? tf('Its list is paused for this version: {why}.', { why: d.paused }) : d.live ? live : '';
    const banner = mastered || beaten ? `<div class="ach-banner ${mastered ? 'gold' : 'silver'}">${STAR(true)}<div><b>${esc(mastered ? t('Mastered') : t('Beaten'))}</b><span>${esc(mastered ? t('Every achievement in this game is yours.') : t('You\'ve beaten this game. Mastered is every achievement.'))}</span></div></div>` : '';
    const players = Number(mine.players) || 0;
    const head = `<div class="card ach-head">${banner}<div class="ach-sum"><div><span class="ach-big">${esc(tf('{n} of {total}', { n: got, total: of }))}</span><span class="muted">${esc([d.total ? tf('{n} of {total} points', { n: d.points, total: d.total }) : t('Achievements'), players ? tn(players, '1 player', '{n} players') : ''].filter(Boolean).join(' · '))}</span></div><button type="button" class="btn sm" data-act="url" data-url="${esc(WEB_A + '/' + encodeURIComponent(id))}">${esc(t('View on the Web'))}</button></div>
      <div class="ach-bar big${mastered ? ' gold' : ''}" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${pct}"><i style="width:${pct}%"></i></div>
      ${status ? `<p class="muted" style="margin:12px 0 0">${esc(status)}</p>` : ''}
      <div class="set-row" style="margin-top:12px"><div class="k"><b>${esc(t('Unlock Sound'))}</b><span>${esc(t('A short chime with each toast over the game.'))}</span></div><button type="button" class="switch" role="switch" aria-checked="${!!d.sound}" aria-label="${esc(t('Unlock Sound'))}" data-ach="sound" data-id="${esc(id)}"></button></div></div>`;
    let body = shown.length
      ? `<div class="card ach-list">${main.map((x) => row(x, d)).join('')}</div>`
      : `<div class="card"><p style="margin:0">${esc(t('No achievements for this game yet.'))}</p><p class="muted" style="margin:6px 0 0">${esc(t('A list for it shows here once there is one. For testing, put one in the achievements\\dev-lists folder as <game id>.json.'))}</p></div>`;
    if (bonus.length) body += `<section class="ach-sec"><div class="ach-sec-h"><h3>${esc(t('Bonus'))}</h3><span class="muted">${esc(tf('{n} of {total}', { n: bonus.filter(has).length, total: bonus.length }))}</span></div><p class="muted ach-sec-p">${esc(t('Bonus achievements don\'t count for Beaten or Mastered.'))}</p><div class="card ach-list">${bonus.map((x) => row(x, d)).join('')}</div></section>`;
    if (boards.length) body += `<section class="ach-sec"><div class="ach-sec-h"><h3>${esc(t('Leaderboards'))}</h3></div>${lbGrid(boards.map((b) => lbCard(id, b, (mine.boards || []).find((x) => String(x.id) === String(b.id)))))}</section>`;
    // An Xbox 360 recomp whose disc the launcher hasn't found yet (it looks while the game runs):
    // the player can point to it.
    const disc = a.origin === 'x360' && !d.native_known && !list.length
      ? `<div class="card"><p style="margin:0"><b>${esc(t('Show This Game\'s Achievements'))}</b></p><p class="muted" style="margin:6px 0 12px">${esc(A.discMsg[id] || t('Point RecompLauncher to your game files to show this game\'s achievements.'))}</p><div class="ach-acts"><button type="button" class="btn" data-ach="disc-file" data-id="${esc(id)}">${esc(t('Choose Disc Image'))}</button><button type="button" class="btn" data-ach="disc-folder" data-id="${esc(id)}">${esc(t('Choose Game Folder'))}</button></div></div>`
      : '';
    return `<div class="stack">${head}${disc}${body}${d.dev || (typeof devOn === 'function' && devOn()) ? devTools(id, d) : ''}</div>`;
  };

  // Report a problem with one achievement: what's wrong, and an optional note.
  function report(game, aid) {
    const d = A.games[game] || {};
    const a = ((d.list || {}).achievements || []).find((x) => Number(x.id) === aid) || {};
    const s = modal(`<h2>${esc(t('Report a Problem'))}</h2><p class="muted">${esc(a.title || tf('Achievement {n}', { n: aid }))}</p>
      <div class="ach-kinds" role="radiogroup" aria-label="${esc(t('What\'s Wrong'))}">${KINDS.map(([k, l], i) => `<label class="ach-kind"><input type="radio" name="ach-kind" value="${k}"${i === 0 ? ' checked' : ''}><span>${esc(t(l))}</span></label>`).join('')}</div>
      <label class="ach-note">${esc(t('Note (Optional)'))}<textarea class="field" id="ach-rnote" maxlength="600" rows="3" placeholder="${esc(t('What happened, and where in the game'))}"></textarea></label>
      <p class="ach-rerr" role="alert"></p>
      <div class="acts"><button type="button" class="btn" data-x="no">${esc(t('Cancel'))}</button><button type="button" class="btn main" data-x="yes">${esc(t('Send Report'))}</button></div>`, async (x, close) => {
      if (x.dataset.x !== 'yes' || s.dataset.busy) return;
      const kind = (($('input[name="ach-kind"]:checked', s) || {}).value) || 'other';
      const note = ($('#ach-rnote', s) || {}).value || '';
      const err = $('.ach-rerr', s);
      s.dataset.busy = '1'; x.disabled = true; x.textContent = t('Sending…');
      let r;
      try { r = await invoke('ach_report', { game, id: aid, kind, note }); } catch (e) { r = { error: String((e && e.message) || e) }; }
      delete s.dataset.busy; x.disabled = false; x.textContent = t('Send Report');
      if (r && r.error) { err.textContent = String(r.error); return; }
      close();
      A.reportedNow[`${game}:${aid}`] = 'open';
      toast(t('Thanks. Your report is in.'));
      if (onTab(game)) refreshGame();
    }, t('Report a Problem'));
  }

  document.addEventListener('click', async (e) => {
    const b = e.target.closest && e.target.closest('[data-ach]');
    if (!b) return;
    const id = b.dataset.id, k = b.dataset.ach;
    if (k === 'report') {
      report(id, Number(b.dataset.aid));
    } else if (k === 'disc-file' || k === 'disc-folder') {
      const folder = k === 'disc-folder';
      let path = null;
      try {
        path = typeof pickFile === 'function'
          ? await pickFile(folder ? { title: t('Your Game Folder'), directory: true } : { title: t('Your Disc Image'), filters: [{ name: t('Xbox 360 Disc Image'), extensions: ['iso', 'xiso', 'xex'] }] })
          : null;
      } catch (err) { path = null; }
      if (!path) return;
      let r = {};
      try { r = await invoke('ach_set_disc', { id, path: String(path) }); } catch (err) { r = { ok: false }; }
      A.discMsg[id] = r.ok ? tf('Found {n} achievements.', { n: r.count }) : t('No Xbox 360 game was found there. Pick the disc image (.iso), or the folder with default.xex in it.');
      load(id, true);
    } else if (k === 'sound') {
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
  Object.assign(A, { status: null, profile: null, list: null, boards: {}, ptab: 'ov', gfilter: 'all', bgame: '', by: 'points', form: null, onPage: false });
  const redraw = () => { if (S.view === 'achievements') render(true); if (window.tvRefresh) window.tvRefresh(); };
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
  async function loadBoard(game, by = A.by) {
    const key = `${game || ''}|${by}`;
    if (game && A.web[game] === undefined) { A.web[game] = null; loadWeb(game); }
    try { const r = await invoke('ach_board', { game: game || null, by }); if (Array.isArray(r.board) && !same(r.board, A.boards[key])) { A.boards[key] = r.board; redraw(); } } catch (e) { /* offline */ }
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
  const cov = (g, cls = '') => { const c = coverOf(g); const k = COVERS[[...String(g.id)].reduce((n, ch) => n + ch.charCodeAt(0), 0) % COVERS.length]; return `<div class="achp-cov ph-${k} ${cls}"><span>${esc(nameOf(g))}</span>${c ? pic(thumbOf(c, cls === 'wide' ? 640 : 400), '', `loading="lazy" data-orig="${esc(media(c))}"`) : ''}</div>`; };
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
  // The team's pinned note from a=status (a title, its text and a button), above everything else.
  const PIN = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M9 4h6l-1 6 3 3H7l3-3zM12 13v7"/></svg>';
  function noteCard() {
    const n = A.status && A.status.note;
    if (!n || typeof n !== 'object' || !(n.title || n.text)) return '';
    const url = typeof n.url === 'string' && /^https:\/\//i.test(n.url) ? n.url : '';
    return `<section class="card achp-pin" role="note"><span class="achp-pin-ic">${PIN}</span><div class="achp-pin-k">${n.title ? `<strong>${esc(n.title)}</strong>` : ''}${String(n.text || '').split(/\n+/).filter(Boolean).map((l) => `<p>${esc(l)}</p>`).join('')}</div>${url ? `<button type="button" class="btn sm main" data-act="url" data-url="${esc(url)}">${EXT}${esc(n.button || t('Open'))}</button>` : ''}</section>`;
  }
  const head = (sub, btn) => `<div class="head achp-head"><div><div class="achp-h"><h1>${esc(t('Achievements'))}</h1><span class="achp-pill">${esc(t('Alpha'))}</span></div><p class="achp-sub">${esc(sub)}</p></div>${btn}</div>`;
  const webBtn = (label) => `<button type="button" class="btn" data-act="url" data-url="${WEB}">${EXT}${esc(label)}</button>`;
  // As many columns as there are cards (up to `max`), so a row never has a lone leftover.
  const cols = (n, max) => (n <= max ? n : n % max === 0 ? max : n % (max - 1) === 0 ? max - 1 : max);
  // Columns for a grid that can't change its count (a list of games): full rows when they divide
  // evenly, else the count whose last row is fullest, never one card alone on it.
  const evenCols = (n, max) => {
    if (n <= max) return Math.max(1, n);
    for (let c = max; c >= 2; c--) if (n % c === 0) return c;
    let best = 2, fill = -1;
    for (let c = max; c >= 2; c--) { const r = n % c; if (r !== 1 && r / c > fill) { fill = r / c; best = c; } }
    return best;
  };

  // Every game with a list, with this player's progress on it.
  function games() {
    const mine = new Map(((A.profile && A.profile.games) || []).map((g) => [g.id, g]));
    // The list is every game with achievements; whether it's Beaten or Mastered is the player's own
    // (their profile), never the list's (its counts are other players').
    const out = (A.list || []).map((g) => {
      const base = Object.assign({}, g);
      delete base.mastered; delete base.beaten;
      return Object.assign({ unlocked: 0, points: 0, points_total: g.points, achievements: g.achievements }, base, mine.get(g.id) || {});
    });
    for (const g of mine.values()) if (!out.some((x) => x.id === g.id)) out.push(g);
    return out;
  }
  const beatenIds = () => new Set(((A.profile && A.profile.beaten) || []).map((b) => b.id));
  const isBeaten = (g, set) => !g.mastered && !!(g.beaten || (set || beatenIds()).has(g.id));
  const isPlaying = (g) => { const pl = A.profile && A.profile.playing; return Array.isArray(pl) ? pl.some((x) => (x && x.id ? x.id : x) === g.id) : g.unlocked > 0 && !g.mastered; };
  // The gold Mastered or silver Beaten tag on a cover.
  const badge = (k) => `<span class="achp-mbadge${k === 'b' ? ' silver' : ''}">${STARP}${esc(k === 'b' ? t('Beaten') : t('Mastered'))}</span>`;

  function pageTester() {
    const p = A.profile || {};
    const u = user() || {};
    const recent = p.recent || [];
    const last = recent[0];
    const line = [p.rank ? tf('Rank #{rank} of {players} players', { rank: fmtN(p.rank), players: fmtN(p.players || 0) }) : t('No Ranked points yet'), last ? tf('Last unlock: {title}, {when}', { title: last.title, when: agoShort(last.at) }) : ''].filter(Boolean).join(' · ');
    const stat = (v, k, small, info) => `<div class="achp-stat"><b>${v}${small ? `<small>${esc(small)}</small>` : ''}</b><span>${esc(k)}${info ? ` <i class="achp-i" title="${esc(info)}" aria-label="${esc(info)}">i</i>` : ''}</span></div>`;
    const rk = (n) => (n ? '#' + fmtN(n) : '');
    const SCORE_NOTE = t('Points weighted by how rare each achievement is.');
    const profBtn = u.username ? `<button type="button" class="btn sm" data-act="url" data-url="${esc(profileUrl(u.username))}">${EXT}${esc(t('View Your Profile on the Web'))}</button>` : '';
    const prof = `<section class="card achp-prof">${typeof avatar === 'function' ? avatar(u, 64) : ''}<div class="achp-prof-n"><h2>${esc(u.username || t('Tester'))} <span class="achp-pill c">${esc(t('Tester'))}</span></h2><span>${esc(line)}</span></div>${profBtn}
      <div class="achp-stats">${stat(fmtN(p.points || 0), t('Points'), rk(p.rank))}${stat(fmtN(p.score || 0), t('Score'), rk(p.score_rank), SCORE_NOTE)}${stat(fmtN(p.beaten_count || (p.beaten || []).length || 0), t('Beaten'))}${stat(fmtN(p.mastered_count || 0), t('Mastered'))}</div>
      <p class="achp-note"><i class="achp-i" aria-hidden="true">i</i><span><b>${esc(t('Score'))}</b> · ${esc(SCORE_NOTE)}</span></p></section>`;
    const tabs = [['ov', t('Overview')], ['games', t('Games')], ['lb', t('Leaderboard')]];
    const tabBar = `<nav class="achp-tabs" role="tablist">${tabs.map(([k, l]) => `<button type="button" role="tab" aria-selected="${A.ptab === k}" class="${A.ptab === k ? 'on' : ''}" data-ach="ptab" data-k="${k}">${esc(l)}</button>`).join('')}</nav>`;
    const body = A.ptab === 'games' ? paneGames() : A.ptab === 'lb' ? paneBoard() : paneOverview();
    return `<div class="page achp">${head(t('Your Recomp Achievements across every game. Thanks for testing: report anything odd on Discord.'), webBtn(t('Leaderboards on the Web')))}${noteCard()}${prof}${tabBar}${body}</div>`;
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
      ? `<div class="card achp-list">${recent.map((r) => `<button type="button" class="achp-un" data-ach="open-game" data-id="${esc(r.game)}"><span class="achp-star">${STARP}</span><div><strong>${esc(r.title || tf('Achievement {n}', { n: r.id }))}</strong><small>${esc([r.name || nameOf({ id: r.game }), r.description, r.rarity != null ? rarityText(r.rarity) : ''].filter(Boolean).join(' · '))}</small></div><span class="achp-pts">${esc(tf('{n} pts', { n: fmtN(r.points || 0) }))}</span>${chip(r.ranked)}<span class="achp-when">${esc(agoShort(r.at))}</span></button>`).join('')}</div>`
      : `<div class="card achp-empty">${STARP}<div><strong>${esc(t('No Unlocks Yet'))}</strong><span>${esc(t('Your unlocks show here the moment you earn them.'))}</span></div></div>`;
    // Beaten and Mastered together: Mastered first, and a game that's both shows only Mastered.
    const ms = (p.mastered || []).map((m) => Object.assign({ kind: 'm' }, m));
    const bs = (p.beaten || []).filter((b) => !ms.some((m) => m.id === b.id)).map((b) => Object.assign({ kind: 'b' }, b));
    const done = ms.concat(bs);
    const shortest = games().filter((g) => g.achievements > 0).sort((a, b) => a.achievements - b.achievements)[0];
    const mast = done.length
      ? `<div class="achp-grid" style="--c:${done.length % 2 === 0 ? 2 : 1}">${done.map((m) => `<button type="button" class="card achp-mcard" data-ach="open-game" data-id="${esc(m.id)}">${cov(m, 'wide')}${badge(m.kind === 'm' ? 'm' : 'b')}<strong>${esc(nameOf(m))}</strong><small>${esc([m.at ? date(m.at * 1000) : '', m.ranked == null ? '' : m.ranked ? t('Ranked') : t('Casual')].filter(Boolean).join(' · '))}</small></button>`).join('')}</div>`
      : `<div class="card achp-empty gold">${STARP}<div><strong>${esc(t('Nothing Beaten Yet'))}</strong><span>${esc(shortest ? tf('Beat a game and it lands here; unlock every achievement and it\'s Mastered. {name} is the shortest list ({n}).', { name: nameOf(shortest), n: shortest.achievements }) : t('Beat a game and it lands here; unlock every achievement and it\'s Mastered.'))}</span></div></div>`;
    const evs = p.events || [];
    const evHtml = evs.length ? `<div class="achp-grid" style="--c:${cols(evs.length, 3)}">${evs.map((e) => `<div class="card achp-ev${e.won ? ' won' : ''}"><span class="achp-ev-ic">${STARP}</span><div><strong>${esc(e.title || '')}</strong>${e.won ? `<span class="achp-ev-done">${esc(t('Finished'))}</span>` : `${bar(Number(e.have) || 0, Number(e.goal) || 0, true)}<small>${esc(tf('{n} of {total}', { n: fmtN(Number(e.have) || 0), total: fmtN(Number(e.goal) || 0) }))}</small>`}</div></div>`).join('')}</div>` : '';
    const sec = (h, inner, link) => `<section class="achp-sec"><div class="achp-sec-h"><h3>${esc(h)}</h3>${link || ''}</div>${inner}</section>`;
    return `<div class="achp-pane">${evHtml ? sec(t('Events'), evHtml) : ''}${sec(t('Keep Going'), keep, `<button type="button" class="achp-link" data-ach="ptab" data-k="games">${esc(t('All Games'))}</button>`)}${sec(t('Recent Unlocks'), rec)}${sec(t('Beaten and Mastered'), mast)}${askCard()}</div>`;
  }
  const askCard = () => `<section class="card achp-ask"><div><strong>${esc(t('Don\'t See a Game You Want?'))}</strong><span>${esc(t('Ask its developer to add Recomp Achievements. It\'s free, and most developers are happy to hear a game is wanted.'))}</span></div><button type="button" class="btn sm" data-act="url" data-url="${WEB}">${esc(t('How Developers Add It'))}</button></section>`;

  function paneGames() {
    const all = games();
    const bset = beatenIds();
    const f = { all: () => true, playing: isPlaying, beaten: (g) => isBeaten(g, bset), mastered: (g) => !!g.mastered, none: (g) => !g.unlocked };
    const counts = Object.fromEntries(Object.keys(f).map((k) => [k, all.filter(f[k]).length]));
    const labels = { all: t('All'), playing: t('Playing'), beaten: t('Beaten'), mastered: t('Mastered'), none: t('Not Started') };
    const list = all.filter(f[A.gfilter] || f.all).sort((a, b) => (b.last || 0) - (a.last || 0) || nameOf(a).localeCompare(nameOf(b)));
    const card = (g) => {
      const inst = !!S.library[g.id], inCat = !!cat(g.id);
      const btn = inst ? `<button type="button" class="btn sm main" data-act="play" data-id="${esc(g.id)}">${esc(t('Play'))}</button>` : inCat ? `<button type="button" class="btn sm" data-act="install" data-id="${esc(g.id)}">${esc(t('Install'))}</button>` : '';
      const info = g.mastered ? tf('{n} of {total} · {kind}', { n: fmtN(g.unlocked), total: fmtN(g.achievements), kind: g.mastered === 'ranked' ? t('Ranked') : t('Casual') }) : tf('{n} of {total} · {pts} pts', { n: fmtN(g.unlocked || 0), total: fmtN(g.achievements || 0), pts: fmtN(g.points || 0) });
      return `<div class="achp-gcard" data-ach="open-game" data-id="${esc(g.id)}" role="button" tabindex="0"><div class="achp-cwrap">${cov(g)}${inst ? `<span class="achp-pill c achp-tagm">${esc(t('Installed'))}</span>` : ''}${g.mastered ? badge('m') : isBeaten(g, bset) ? badge('b') : ''}</div><strong>${esc(nameOf(g))}</strong>${bar(g.unlocked, g.achievements, g.mastered)}<div class="achp-row"><span>${esc(info)}</span>${btn}</div></div>`;
    };
    return `<div class="achp-pane"><p class="muted achp-sec-p">${esc(t('Every Xbox 360 game is open: the first time a tester plays one, its achievements are added for everyone.'))}</p><div class="achp-top"><div class="chips">${Object.keys(f).map((k) => `<button type="button" class="chip${A.gfilter === k ? ' on' : ''}" data-ach="gfilter" data-k="${k}">${esc(labels[k])} ${fmtN(counts[k])}</button>`).join('')}</div></div>
      ${list.length ? `<div class="achp-games" style="--c:${evenCols(list.length, 5)};--cn:${evenCols(list.length, 4)}">${list.map(card).join('')}</div>` : `<div class="card achp-empty">${STARP}<div><strong>${esc(t('Nothing Here'))}</strong><span>${esc(all.length ? t('No games match this filter yet.') : t('Xbox 360 games appear here as testers play them.'))}</span></div></div>`}</div>`;
  }

  function paneBoard() {
    const rows = A.boards[`${A.bgame}|${A.by}`] || null;
    const me = (user() || {}).username;
    const score = A.by === 'score';
    const opts = [['', t('All Games')], ...games().map((g) => [g.id, nameOf(g)])];
    const picker = `<select class="sel" data-ach="bgame" aria-label="${esc(t('Game'))}">${opts.map(([v, l]) => `<option value="${esc(v)}"${v === A.bgame ? ' selected' : ''}>${esc(l)}</option>`).join('')}</select>`;
    const sw = `<div class="seg achp-seg" role="tablist" aria-label="${esc(t('Rank By'))}">${[['points', t('Points')], ['score', t('Score')]].map(([k, l]) => `<button type="button" role="tab" aria-selected="${A.by === k}" class="${A.by === k ? 'on' : ''}" data-ach="by" data-k="${k}">${esc(l)}</button>`).join('')}</div>`;
    const name = (r, you) => r.username ? `<button type="button" class="achp-pname" data-act="url" data-url="${esc(profileUrl(r.username))}">${esc(r.username)}</button>${you ? ` <em>· ${esc(t('You'))}</em>` : ''}` : '';
    const row = (r, you) => `<div class="achp-lr${you ? ' you' : ''}"><span class="achp-rk">${esc(String(r.rank || '—'))}</span>${typeof avatar === 'function' ? avatar({ username: r.username, avatar: r.avatar }, 30) : ''}<strong>${name(r, you)}</strong><span class="achp-num">${fmtN((score ? r.score : r.points) || 0)}</span><span class="achp-num">${fmtN(r.unlocks || 0)}</span><span class="achp-num">${fmtN(r.mastered || 0)}</span></div>`;
    let body;
    if (!rows) body = `<p class="muted" style="margin:12px">${esc(t('Loading the leaderboard…'))}</p>`;
    else if (!rows.length) body = `<p class="muted" style="margin:12px">${esc(t('Nobody has Ranked points here yet.'))}</p>`;
    else {
      body = rows.map((r) => row(r, me && r.username === me)).join('');
      const p = A.profile || {};
      const myRank = score ? p.score_rank : p.rank;
      if (me && !A.bgame && !rows.some((r) => r.username === me) && myRank) body += `<div class="achp-lr gap"><span>⋯</span></div>` + row({ rank: myRank, username: me, avatar: (user() || {}).avatar, points: p.points, score: p.score, unlocks: p.unlocks, mastered: p.mastered_count }, true);
    }
    // A game's own leaderboards, under its top players.
    let gameBoards = '';
    if (A.bgame) {
      const web = A.web[A.bgame];
      const list = (web && Array.isArray(web.leaderboards)) ? web.leaderboards : [];
      const mineB = ((A.games[A.bgame] || {}).mine || {}).boards || [];
      if (list.length) gameBoards = `<section class="achp-sec"><div class="achp-sec-h"><h3>${esc(t('Leaderboards'))}</h3></div>${lbGrid(list.map((b) => lbCard(A.bgame, b, mineB.find((x) => String(x.id) === String(b.id)))))}</section>`;
    }
    return `<div class="achp-pane"><div class="achp-top"><span class="muted">${esc(score ? t('Score is Points weighted by how rare each achievement is. Only Ranked unlocks count.') : t('Only Ranked points count.'))}</span><div class="achp-tools">${sw}${picker}</div></div>
      <div class="card achp-lb"><div class="achp-lr hd"><span>#</span><span></span><span>${esc(t('Player'))}</span><span class="achp-num">${esc(score ? t('Score') : t('Points'))}</span><span class="achp-num">${esc(t('Unlocks'))}</span><span class="achp-num">${esc(t('Mastered'))}</span></div>${body}</div>${gameBoards}</div>`;
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
    const gamesSec = `<section class="achp-sec"><div class="achp-sec-h"><h3>${esc(t('Games with Achievements'))}</h3></div><p class="muted achp-sec-p">${esc(t('Every Xbox 360 game is open: the first time a tester plays one, its achievements are added for everyone.'))}</p>
      ${list.length ? `<div class="achp-grid" style="--c:${list.length + 1}">${list.map(mini).join('')}<div class="achp-mini"><div class="achp-cov dash"><span>${esc(t('Your Game?'))}</span></div><strong>${esc(t('Don\'t See a Game?'))}</strong><small>${esc(t('Ask its developer to add it. It\'s free.'))}</small></div></div>` : `<div class="card achp-empty">${STARP}<div><span>${esc(t('Xbox 360 games appear here as testers play them.'))}</span></div></div>`}</section>`;
    return `<div class="page achp">${head(t('Earn achievements in your recomps, track them on your profile and climb the leaderboards.'), webBtn(t('Learn More')))}${noteCard()}${hero}${featHtml}${block}${gamesSec}</div>`;
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
    else if (k === 'by') { A.by = b.dataset.k === 'score' ? 'score' : 'points'; redraw(); loadBoard(A.bgame); }
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
  // While the page is open, the status is asked again every few minutes (the core only goes to the
  // website when its copy is old), so a note the team posts turns up without a restart.
  setInterval(() => { if (S.view === 'achievements' && !document.hidden) loadStatus(false); }, 4 * 60 * 1000);

  T.event.listen('ach-access', ({ payload }) => { A.allowed = !!(payload && payload.allowed); if (S.view === 'game') refreshGame(); loadStatus(false); });
  T.event.listen('ach-changed', ({ payload }) => { const id = payload && payload.game; if (id && (A.games[id] || onTab(id))) load(id); });
  T.event.listen('ach-log', ({ payload }) => { const id = payload && payload.game; if (!id || !onTab(id)) return; clearTimeout(logTimer); logTimer = setTimeout(() => loadLog(id), 250); });
  T.event.listen('account', () => setTimeout(() => { A.profile = null; A.form = null; loadStatus(true); }, 800));
  T.event.listen('game', ({ payload }) => { const id = payload && payload.id; if (id && A.allowed) setTimeout(() => load(id), payload.state === 'running' ? 1500 : 300); });

  // What Big Picture mode shows (tv.js draws it, sized for the TV; reporting stays here).
  window.achTV = {
    A,
    fmtValue,
    rarityText,
    isBonus,
    topOf,
    nameIn,
    game(id) { if (!A.allowed) return null; if (A.games[id] === undefined) { A.games[id] = null; load(id, true); } const d = A.games[id]; if (d) { d.id = id; const b = d.list && Array.isArray(d.list.leaderboards) ? d.list.leaderboards : []; if (b.length && A.web[id] === undefined) { A.web[id] = null; loadWeb(id); } } return d; },
    page() { if (!A.tvAsked) { A.tvAsked = true; loadStatus(true); if (A.allowed) loadBoard('', 'points'); } return { status: A.status, tester: !!(A.status && A.status.tester && A.allowed), profile: A.profile, board: A.boards['|points'] || null, games: games() }; },
    left() { A.tvAsked = false; },
  };
})();
