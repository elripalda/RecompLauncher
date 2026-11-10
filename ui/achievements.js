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
    const pct = shown.length ? Math.round((got / shown.length) * 100) : 0;
    const status = d.paused ? tf('Its list is paused for this version: {why}.', { why: d.paused }) : d.live ? (d.connected ? t('The game is connected: unlocks show as they happen.') : t('The game is running. It hasn\'t connected (it may not have achievements built in yet).')) : '';
    const head = `<div class="card ach-head"><div class="ach-sum"><div><span class="ach-big">${esc(tf('{n} of {total}', { n: got, total: shown.length }))}</span><span class="muted">${esc(d.total ? tf('{n} of {total} points', { n: d.points, total: d.total }) : t('Achievements'))}</span></div>${d.mastered ? `<span class="ach-mastered">${esc(t('Mastered'))}</span>` : ''}</div>
      <div class="ach-bar big" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${pct}"><i style="width:${pct}%"></i></div>
      ${status ? `<p class="muted" style="margin:12px 0 0">${esc(status)}</p>` : ''}
      <div class="set-row" style="margin-top:12px"><div class="k"><b>${esc(t('Unlock Sound'))}</b><span>${esc(t('A short chime with each toast over the game.'))}</span></div><button type="button" class="switch" role="switch" aria-checked="${!!d.sound}" aria-label="${esc(t('Unlock Sound'))}" data-ach="sound" data-id="${esc(id)}"></button></div></div>`;
    const body = shown.length
      ? `<div class="card ach-list">${shown.map((x) => row(x, d)).join('')}</div>`
      : `<div class="card"><p style="margin:0">${esc(t('No achievements for this game yet.'))}</p><p class="muted" style="margin:6px 0 0">${esc(t('A list for it shows here once there is one. For testing, put one in the achievements\\dev-lists folder as <game id>.json.'))}</p></div>`;
    return `<div class="stack">${head}${body}${devTools(id, d)}</div>`;
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

  T.event.listen('ach-access', ({ payload }) => { A.allowed = !!(payload && payload.allowed); if (S.view === 'game') refreshGame(); });
  T.event.listen('ach-changed', ({ payload }) => { const id = payload && payload.game; if (id && (A.games[id] || onTab(id))) load(id); });
  T.event.listen('ach-log', ({ payload }) => { const id = payload && payload.game; if (!id || !onTab(id)) return; clearTimeout(logTimer); logTimer = setTimeout(() => loadLog(id), 250); });
  T.event.listen('account', () => setTimeout(() => access(true), 800));
  T.event.listen('game', ({ payload }) => { const id = payload && payload.id; if (id && A.allowed) setTimeout(() => load(id), payload.state === 'running' ? 1500 : 300); });
  setTimeout(() => access(true), 1200);
})();
