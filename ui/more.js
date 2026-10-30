'use strict';
// RecompLauncher's own updates (found while it runs, installed quietly, What's new after), games
// sent from recomplauncher.com ("Get it on my PC"), betas of recomps, "How did it run?" after a
// session, and usage and diagnostics. Uses app.js's helpers and state (S), and account.js's ask().

S.betas = [];
const tvOn = () => document.body.classList.contains('tv-on');

// ---------- updates ----------
T.event.listen('launcher-update', ({ payload: v }) => { if (v && v.version) { S.launcherUpd = v; renderSide(); if (S.view === 'downloads') render(true); } if (v && v.version && v.version !== LL.told) { LL.told = v.version; updateToast(v); } });
// Pushed by the website the moment it happens: a recomp's new release (its releases were just
// looked at again), or a changed list.
T.event.listen('push', ({ payload: p }) => {
  if (p && p.type === 'game') checkUpdates();
  else if (p && p.type === 'catalog') loadCatalog(true).then(() => render(true)).catch(() => {});
});
// The download, then the installer's own small window; RecompLauncher opens again after.
let updBox = null;
T.event.listen('self-update', ({ payload: p }) => {
  if (!updBox || !updBox.isConnected) {
    updBox = modal(`<div class="upd"><h2>${esc(t('Updating RecompLauncher'))}</h2><div class="prog"><div class="track"><i style="width:0%"></i></div></div><p class="muted" data-st>${esc(t('Downloading…'))}</p></div>`, () => {}, t('Updating RecompLauncher'));
  }
  const pct = p.total ? Math.round((p.done / p.total) * 100) : 0;
  const bar = $('.track i', updBox), st = $('[data-st]', updBox);
  if (bar) bar.style.width = (p.installing ? 100 : pct) + '%';
  if (st) st.textContent = p.installing ? t('Installing. RecompLauncher opens again in a moment.') : p.total ? tf('{done} of {total}', { done: fmtB(p.done), total: fmtB(p.total) }) : t('Downloading…');
});
// What's new: once, the first time a new version opens.
async function whatsNew() {
  const ui = S.settings.ui || {};
  if (ui.seen_version === S.version) return;
  // A new install has nothing to compare with: just remember this version.
  if (!ui.seen_version && !S.settings.setup_done) return saveUi({ seen_version: S.version });
  // Downloads > Updates lists the launcher's own updates too.
  if (ui.seen_version && !(ui.self_updates || []).some((u) => u.to === S.version)) await saveUi({ self_updates: [{ from: ui.seen_version, to: S.version, when: Math.floor(Date.now() / 1000) }].concat(ui.self_updates || []).slice(0, 20) }, true);
  let v = null;
  try { v = await invoke('launcher_notes'); } catch (e) { /* offline: next time */ return; }
  await saveUi({ seen_version: S.version });
  if (!v || v.version !== S.version || !v.notes) return;
  modal(`<div class="whatsnew"><h2>${esc(tf('What\'s new in RecompLauncher {version}', { version: verName(S.version) }))}</h2><div class="md">${md(v.notes)}</div>
    <div class="acts"><button type="button" class="btn main" data-x="no">${esc(t('Got it'))}</button></div></div>`, () => {}, t('What\'s new'));
}

// ---------- Get it on my PC ----------
T.event.listen('inbox', async ({ payload }) => {
  for (const it of (payload && payload.items) || []) {
    const id = it.game, a = byId(id) || { name: it.name || id };
    let result = 'installing';
    if (S.library[id]) { result = 'installed'; toast(tf('{name} is already installed. Sent from recomplauncher.com.', { name: a.name })); }
    else if (!byId(id) || status(byId(id)) === 'nobuild' || status(byId(id)) === 'soon') { result = 'no'; toast(tf('{name} can\'t be installed on this PC.', { name: a.name }), true); }
    else if (S.jobs[id]) result = 'installing';
    else {
      toast(tf('Installing {name}. Sent from recomplauncher.com.', { name: a.name }));
      startInstall(id, S.settings.library_dir);
    }
    invoke('inbox_done', { id: it.id, result }).catch(() => {});
  }
});

// ---------- betas ----------
async function loadBetas() {
  if (!(S.account && S.account.signedIn)) { S.betas = []; return; }
  try { S.betas = (await invoke('betas')).betas || []; } catch (e) { return; }
  if (S.view === 'game') refreshGame();
}
const betaOf = (id) => (S.betas || []).find((b) => b.app === id && (b.systems || []).includes(S.os));
// The build a beta plays: its tag, or the newest pre-release that has a file for this PC.
async function betaTag(b) {
  if (b.channel === 'tag' && b.tag) return b.tag;
  const vs = await invoke('versions', { id: b.app });
  const pre = vs.find((v) => v.pre);
  if (!pre) throw new Error(t('There\'s no test build for this PC yet.'));
  return pre.tag;
}
function onBetaNow(id, b) {
  const g = S.library[id], mark = ((S.settings.ui || {}).beta_tags || {})[id];
  return !!g && b.status === 'in' && (b.channel === 'tag' && b.tag ? g.tag === b.tag : mark ? g.tag === mark : false);
}
function betaCard(a) {
  const b = betaOf(a.id);
  if (!b) return '';
  const g = S.library[a.id], onBeta = onBetaNow(a.id, b);
  const head = `<div class="card beta-card"><div class="card-h"><h3>${esc(t('Beta'))}${b.title ? ': ' + esc(b.title) : ''}</h3><span class="beta-pill">${esc(b.status === 'in' ? t('You\'re in') : b.status === 'asked' ? t('Asked') : t('Open'))}</span></div>`;
  const notes = b.notes ? `<p class="beta-notes">${esc(b.notes)}</p>` : '';
  let acts;
  if (b.status === 'in') {
    acts = onBeta ? `<p class="muted">${esc(tf('You\'re playing the beta ({tag}).', { tag: g.tag }))}</p><div class="acts grid2"><button type="button" class="btn" data-act="beta-leave" data-id="${esc(a.id)}">${esc(t('Leave the beta'))}</button><button type="button" class="btn main" data-act="beta-off" data-id="${esc(a.id)}">${esc(t('Back to the release'))}</button></div>`
      : `<div class="acts grid2"><button type="button" class="btn" data-act="beta-leave" data-id="${esc(a.id)}">${esc(t('Leave the beta'))}</button><button type="button" class="btn main" data-act="beta-play" data-id="${esc(a.id)}">${esc(g ? t('Play the beta') : t('Install the beta'))}</button></div>`;
  } else if (b.status === 'asked') {
    acts = `<p class="muted">${esc(t('You asked to join. You can play it here once you\'re let in.'))}</p>`;
  } else {
    acts = `<p class="muted">${esc(b.access === 'request' ? t('Ask to join, and you can play its test build here once you\'re let in.') : t('Join to play its test build. Going back to the normal release is one button.'))}</p><div class="acts"><button type="button" class="btn main" data-act="beta-join" data-id="${esc(a.id)}">${esc(b.access === 'request' ? t('Ask to join') : t('Join the beta'))}</button></div>`;
  }
  return head + notes + acts + '</div>';
}
window.betaCard = betaCard;
// Big Picture: the beta in a game's More sheet.
window.tvBetaItem = (id) => {
  const b = betaOf(id);
  if (!b) return null;
  const g = S.library[id], onBeta = onBetaNow(id, b);
  const el = { dataset: { id } };
  if (b.status === 'asked') return { label: t('Beta: asked to join'), sub: t('You can play it here once you\'re let in'), icon: 'check', run: () => {} };
  if (b.status !== 'in') return { label: b.access === 'request' ? t('Ask to join the beta') : t('Join the beta'), sub: b.title || t('Play its test build'), icon: 'check', run: () => window.moreAct('beta-join', el) };
  return onBeta ? { label: t('Back to the release'), sub: t('Leaves the test build for the normal one'), icon: 'history', run: () => window.moreAct('beta-off', el) }
    : { label: g ? t('Play the beta') : t('Install the beta'), sub: b.title || t('Its test build'), icon: 'check', run: () => window.moreAct('beta-play', el) };
};
// Settings, Account: the betas this account is in or asked for, and open ones for this PC.
window.betasGroup = (group, row) => {
  const list = (S.betas || []).filter((b) => (b.systems || []).includes(S.os));
  if (!list.length) return group(t('Betas'), `<p class="set-empty">${esc(t('No recomp has a beta for this PC right now. When one does, it shows here and on the game\'s page.'))}</p>`);
  const word = { in: N_('You\'re in'), asked: N_('Asked'), open: N_('Open to join') };
  return group(t('Betas'), list.map((b) => row(b.name + (b.title ? ' · ' + b.title : ''), t(word[b.status] || word.open), `<button type="button" class="btn sm" data-act="open" data-id="${esc(b.app)}">${esc(t('Go to the game'))}</button>`)).join(''));
};
window.moreAct = async (act, el) => {
  const id = el.dataset.id, b = betaOf(id);
  switch (act) {
    case 'beta-join': {
      if (!b) return;
      try { const r = await invoke('beta_join', { id: b.id }); S.betas = r.betas || S.betas; toast(r.status === 'in' ? t('You\'re in the beta.') : t('Asked. You\'ll be able to play it once you\'re let in.')); } catch (e) { fail(e); }
      return refreshGame();
    }
    case 'beta-leave': {
      if (!b) return;
      const was = onBetaNow(id, b);
      try { const r = await invoke('beta_leave', { id: b.id }); S.betas = r.betas || []; } catch (e) { return fail(e); }
      if (!was) { toast(t('You left the beta.')); return refreshGame(); }
      // Off the test build too.
      const bt = Object.assign({}, (S.settings.ui || {}).beta_tags || {});
      delete bt[id];
      await saveUi({ beta_tags: bt });
      toast(t('You left the beta. Going back to the release.'));
      return startInstall(id, null, null);
    }
    case 'beta-play': {
      if (!b) return;
      let tag;
      try { tag = await betaTag(b); } catch (e) { return fail(e); }
      await saveUi({ beta_tags: Object.assign({}, (S.settings.ui || {}).beta_tags || {}, { [id]: tag }) });
      toast(tf('Getting the beta of {name} ({tag}).', { name: b.name, tag }));
      return startInstall(id, S.library[id] ? null : S.settings.library_dir, tag);
    }
    case 'beta-off': {
      const bt = Object.assign({}, (S.settings.ui || {}).beta_tags || {});
      delete bt[id];
      await saveUi({ beta_tags: bt });
      toast(t('Going back to the release.'));
      return startInstall(id, null, null);
    }
  }
  return null;
};

// ---------- How did it run? ----------
const VERDICT = [['runs', N_('Plays great')], ['issues', N_('Has problems')], ['broken', N_('Won\'t run')]];
async function askRun(id, secs, asked_for) {
  const g = S.library[id];
  if (!g || g.local) return;
  // Asked after the first long session of each version. Not answered: asked again after three more
  // long sessions, three times at most. Answered: not again until the next version.
  const asked = (S.settings.ui || {}).ran || {};
  if (asked[id] === 'never' && !asked_for) return;
  const was = asked[id];
  let r = was && typeof was === 'object' ? Object.assign({}, was) : was ? { tag: was, asks: 1, waits: 0, sent: false } : null;
  if (!r || r.tag !== g.tag) r = { tag: g.tag, asks: 0, waits: 0, sent: false };
  const keep = (x) => saveUi({ ran: Object.assign({}, (S.settings.ui || {}).ran || {}, { [id]: x }) }, true);
  if (!asked_for) {
    if (r.sent || r.asks >= 3) return;
    if (r.asks > 0 && r.waits < 3) { r.waits++; return keep(r); }
    r.asks++; r.waits = 0;
    await keep(r);
  }
  const mins = Math.round(secs / 60);
  const send = async (verdict, note) => {
    try { await invoke('run_report', { id, verdict, note: note || null, mins }); keep(Object.assign({}, r, { sent: true })); toast(t('Thanks! It\'s on the game\'s page on recomplauncher.com.')); delete S.reports[id]; if (S.view === 'game' && S.arg === id) setTimeout(() => loadReports(id, true), 1500); } catch (e) { fail(e); }
  };
  if (tvOn() && window.tvAsk) {
    const i = await window.tvAsk(tf('How did {name} run?', { name: g.name }), VERDICT.map(([, n]) => ({ label: t(n), sub: t('Shows on its page, with your PC\'s graphics card'), icon: 'check' })).concat([{ label: t('Not now'), icon: 'chevL' }]));
    if (i != null && i >= 0 && i < 3) send(VERDICT[i][0]);
    return;
  }
  const old = $('.ask-run');
  if (old) old.remove();
  const box = document.createElement('div');
  box.className = 'ask-run';
  box.setAttribute('role', 'dialog');
  box.innerHTML = `<button type="button" class="ask-x" aria-label="${esc(t('Close'))}">×</button><b>${esc(tf('How did {name} run?', { name: g.name }))}</b>
    <span>${esc(t('It helps others, and shows on its page with your PC\'s graphics card and Windows version.'))}</span>
    <div class="ask-three">${VERDICT.map(([k, n]) => `<button type="button" class="btn v-${k}" data-v="${k}">${esc(t(n))}</button>`).join('')}</div>`;
  document.body.append(box);
  const close = () => { box.classList.add('out'); setTimeout(() => box.remove(), 250); };
  const timer = setTimeout(close, 60000);
  box.addEventListener('click', (e) => {
    if (e.target.closest('.ask-x')) { clearTimeout(timer); return close(); }
    const v = e.target.closest('[data-v]');
    if (v) {
      clearTimeout(timer);
      // A few words, if they want: then it's sent.
      box.innerHTML = `<b>${esc(t('Anything others should know?'))}</b><textarea rows="3" maxlength="500" placeholder="${esc(t('What works, what doesn\'t, settings that helped'))}"></textarea><div class="acts grid2"><button type="button" class="btn" data-s="skip">${esc(t('Just send it'))}</button><button type="button" class="btn main" data-s="note">${esc(t('Send'))}</button></div>`;
      const ta = $('textarea', box);
      ta.focus();
      box.onclick = (ev) => { const s = ev.target.closest('[data-s]'); if (!s) return; send(v.dataset.v, s.dataset.s === 'note' ? ta.value.trim() : ''); close(); };
    }
  });
}
T.event.listen('game', ({ payload: p }) => {
  if (p.state !== 'stopped' || (p.code != null && p.code !== 0)) return;
  if ((p.secs || 0) >= 600) setTimeout(() => askRun(p.id, p.secs), 1500);
});

// ---------- usage ----------
// Installs and updates that finished or failed (the version once the Library has caught up).
T.event.listen('install', ({ payload: p }) => {
  if (!['done', 'error'].includes(p.phase)) return;
  const had = S.library[p.id] ? S.library[p.id].tag : '', msg = p.phase === 'error' ? String(p.msg || '').replace(/^BLOCKED\|[^|]*\|/, '') : null;
  setTimeout(() => {
    const now = S.library[p.id] ? S.library[p.id].tag : had;
    invoke('usage_event', { kind: p.phase === 'error' ? 'error' : had ? 'update' : 'install', game: p.id, version: now || null, detail: msg }).catch(() => {});
  }, 2500);
});
window.usageUninstall = (id) => invoke('usage_event', { kind: 'uninstall', game: id, version: null, detail: null }).catch(() => {});
// Big Picture mode was used (for how many play on a TV).
let statsMode = '';
new MutationObserver(() => {
  if (tvOn() && !(S.settings.ui || {}).bp_used) saveUi({ bp_used: true });
  const m = tvOn() ? 'tv' : 'desk';
  if (m !== statsMode) { statsMode = m; invoke('usage_mode', { mode: m }).catch(() => {}); }
}).observe(document.body, { attributes: true, attributeFilter: ['class'] });

// ---------- start ----------
setTimeout(async () => {
  const lang = (S.settings.ui || {}).lang;
  const screenSize = `${Math.round(screen.width * (devicePixelRatio || 1))}x${Math.round(screen.height * (devicePixelRatio || 1))}`;
  invoke('usage_hello', { lang: !lang || lang === 'auto' ? navigator.language : lang, screen: screenSize }).catch(() => {});
}, 15000);
setTimeout(() => { statsMode = tvOn() ? 'tv' : 'desk'; invoke('usage_open', { mode: statsMode }).catch(() => {}); }, 2500);
setTimeout(whatsNew, 3000);
setTimeout(loadBetas, 4000);
setInterval(loadBetas, 30 * 60 * 1000);
T.event.listen('account', () => setTimeout(loadBetas, 1500));
window.askRun = askRun;
