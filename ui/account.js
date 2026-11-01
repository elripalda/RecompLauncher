'use strict';
// The RecompStore account and cloud saves: signing in (browser or phone), Settings > Account, the
// sidebar's account button, a game's Saves tab, and what happens before Play (turn cloud saves on?
// a newer save from another PC? which save to keep?). Works the same in Big Picture mode, with its
// own sheets. Uses app.js's helpers and state (S).

const ACC_ICON = '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="8.5" r="3.6"/><path d="M4.8 20a7.2 7.2 0 0 1 14.4 0"/></svg>';
const CLOUD_ICON = '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M7 18a4.5 4.5 0 0 1-.6-9 6 6 0 0 1 11.5 1.6A3.8 3.8 0 0 1 17.5 18z"/></svg>';
const SITE = 'https://recomplauncher.com';
S.account = { signedIn: false };
S.saves = {};

const whenText = (ts) => (ts ? new Date(ts * 1000).toLocaleString(curLocale(), { month: 'short', day: 'numeric', year: 'numeric', hour: 'numeric', minute: '2-digit' }) : '');
const avatar = (u, size = 28) => (u && u.avatar ? `<img class="acc-pic" src="${esc(u.avatar)}" alt="" width="${size}" height="${size}">` : `<span class="acc-pic none" style="width:${size}px;height:${size}px">${esc(((u && u.username) || '?').slice(0, 1).toUpperCase())}</span>`);
const inTV = () => document.body.classList.contains('tv-on');

async function loadAccount(fresh) {
  const was = !!(S.account && S.account.signedIn);
  try { S.account = await invoke('account_state', { fresh: !!fresh }); } catch (e) { S.account = S.account || { signedIn: false }; }
  // Signed in (or out) just now: what each game's Saves tab knew is out of date.
  if (was !== !!S.account.signedIn) { S.saves = {}; S.savesAll = null; }
  renderAccountBits();
}
function renderAccountBits() {
  const b = $('#acc-btn');
  if (b) {
    const a = S.account || {};
    b.innerHTML = a.signedIn ? `${avatar(a.user, 22)}<span class="lbl">${esc((a.user && a.user.username) || t('Account'))}</span>` : `${ACC_ICON}<span class="lbl">${esc(t('Sign in'))}</span>`;
    b.title = a.signedIn ? t('Account') : t('Sign in');
  }
  if (S.view === 'settings' && S.setTab === 'account' && window.setSection) window.setSection('account', false);
  // A game's page (its Saves tab, the cloud card) and the Saves page show it straight away.
  if (S.view === 'game') refreshGame();
  if (S.view === 'saves' || S.view === 'achievements') render(true);
  if (window.tvRefresh && inTV()) window.tvRefresh();
  if (window.oobRedraw) window.oobRedraw();
}

// ---------- signing in ----------
const SIGN = { timer: null, sheet: null };
function stopSignIn() {
  clearTimeout(SIGN.timer); SIGN.timer = null;
  invoke('account_cancel').catch(() => {});
}
// Desktop: the browser opens on the page that says yes, and the code is shown in case it's needed
// on another device. Big Picture mode: the code and a QR code for a phone.
async function signIn(create) {
  let s;
  try { s = await invoke('account_start', { create: !!create }); } catch (e) { return fail(e); }
  const tv = inTV() && window.tvSignIn;
  let box, setStatus;
  const done = (user) => {
    clearTimeout(SIGN.timer); SIGN.timer = null;
    if (SIGN.sheet) { SIGN.sheet.close(true); SIGN.sheet = null; }
    if (box) box.remove();
    toast(tf('Signed in as {name}.', { name: (user && user.username) || '' }));
    loadAccount(true);
  };
  if (tv) {
    SIGN.sheet = window.tvSignIn({ qr: s.qr, code: s.code, onClose: stopSignIn });
    setStatus = (m) => SIGN.sheet && SIGN.sheet.say(m);
  } else {
    invoke('open_url', { url: s.url }).catch(() => {});
    box = modal(`<div class="signin"><h2>${esc(create ? t('Create your account') : t('Sign in'))}</h2>
      <p>${esc(t('Your browser opened recomplauncher.com. Sign in there and press Sign in to connect this PC.'))}</p>
      <div class="signin-two"><div class="signin-qr" aria-hidden="true">${s.qr}</div><div class="signin-code"><small>${esc(t('On another device? Scan the code, or go to recomplauncher.com/activate and type:'))}</small><b>${esc(s.code)}</b></div></div>
      <p class="signin-st" data-st><span class="spinr"></span>${esc(t('Waiting for you to sign in…'))}</p>
      <div class="acts grid2"><button type="button" class="btn" data-x="no">${esc(t('Cancel'))}</button><button type="button" class="btn main" data-x="open">${esc(t('Open the browser again'))}</button></div></div>`, (x) => {
      if (x.dataset.x === 'open') invoke('open_url', { url: s.url }).catch(fail);
      if (x.dataset.x === 'again') { box.remove(); signIn(create); }
    }, t('Sign in'));
    // Closed some other way (Cancel, Esc, clicking outside): stop waiting.
    new MutationObserver((m, o) => { if (!box.isConnected) { o.disconnect(); if (SIGN.timer) stopSignIn(); } }).observe(document.body, { childList: true });
    setStatus = (m, again) => { const st = $('[data-st]', box); if (st) st.innerHTML = esc(m) + (again ? ` <button type="button" class="linkish" data-x="again">${esc(t('Get a new code'))}</button>` : ''); };
  }
  const until = Date.now() + s.expires * 1000;
  const tick = async () => {
    SIGN.timer = null;
    let r;
    try { r = await invoke('account_poll'); } catch (e) { r = { status: 'pending' }; }
    if (!box && !SIGN.sheet) return;
    if (box && !box.isConnected) return;
    if (r.status === 'ok') return done(r.user);
    if (r.status === 'denied') return setStatus(t('Not signed in. You said no on the website.'), true);
    if (r.status === 'expired' || Date.now() > until) return setStatus(t('That code ran out.'), true);
    SIGN.timer = setTimeout(tick, s.interval * 1000);
  };
  SIGN.timer = setTimeout(tick, s.interval * 1000);
}

async function signOut() {
  try { await invoke('account_signout'); } catch (e) { /* signed out here anyway */ }
  S.account = { signedIn: false };
  S.saves = {};
  toast(t('Signed out. Your saves stay on this PC.'));
  renderAccountBits();
}

// ---------- Settings > Account ----------
function accountSection(group, row) {
  const a = S.account || {};
  if (!a.signedIn) {
    return group(t('Account'), `<div class="acc-hero">${CLOUD_ICON}<div><b>${esc(t('Keep your saves safe'))}</b><span>${esc(t('With a RecompStore account, your saves go to the cloud after you play and are there on any PC. It\'s free, and the launcher works the same without one.'))}</span></div></div>
      <div class="acts grid2 acc-acts"><button type="button" class="btn" data-act="acc-create">${esc(t('Create an account'))}</button><button type="button" class="btn main" data-act="acc-signin">${esc(t('Sign in'))}</button></div>`)
      + group(t('Save backups'), row(t('On this PC'), t('After every session the launcher keeps a copy of each game\'s saves, the newest ten. They\'re on each game\'s Saves tab.'), ''));
  }
  const used = a.used || 0, quota = a.quota || 150 * 1024 * 1024, pct = Math.min(100, Math.round((used / quota) * 100));
  return group(t('Account'), `<div class="acc-me">${avatar(a.user, 48)}<div><b>${esc(a.user.username)}</b><span>${esc(a.offline ? t('Signed in on this PC. The website can\'t be reached right now.') : t('Signed in on this PC'))}${a.offline && a.why ? `<small class="muted" style="display:block">${esc(a.why)}</small>` : ''}</span></div>${a.offline ? `<button type="button" class="btn sm" data-act="acc-retry">${esc(t('Try again'))}</button>` : ''}</div>
      <div class="acts grid2 acc-acts"><button type="button" class="btn" data-act="url" data-url="${SITE}/account">${esc(t('Your account'))}</button><button type="button" class="btn" data-act="acc-signout">${esc(t('Sign out'))}</button></div>`)
    + group(t('Cloud saves'), `<div class="acc-space"><div class="acc-bar"><i style="width:${pct}%"></i></div><span>${esc(tf('{used} of {quota} used', { used: fmtB(used) || '0 KB', quota: fmtB(quota) }))}</span></div>
      <p class="set-note" style="margin:12px 0 0">${esc(t('Each game asks the first time you play it. Turn it on or off on a game\'s Saves tab. The newest five copies of each game are kept.'))}</p>`)
    + (window.betasGroup ? window.betasGroup(group, row) : '');
}

// ---------- a game's Saves tab ----------
async function loadSaves(id) {
  S.saves[id] = S.saves[id] && S.saves[id] !== 'loading' ? Object.assign(S.saves[id], { busy: true }) : 'loading';
  try { S.saves[id] = await invoke('saves_info', { id }); } catch (e) { S.saves[id] = { error: String(e && e.message ? e.message : e) }; }
  if (S.view === 'game' && S.arg === id && S.tab === 'saves') refreshGame();
}
const FROM = { player: N_('You picked this folder'), developer: N_('Its developer says it saves here'), catalog: N_('RecompLauncher knows it saves here'), found: N_('Found while you played'), game: N_('In the game\'s folder') };
function savesTab(a) {
  const id = a.id, s = S.saves[id], g = S.library[id] || {};
  if (!s || s === 'loading') { if (!s) loadSaves(id); return `<div class="stack"><div class="card"><p class="muted" style="margin:0">${esc(t('Loading…'))}</p></div></div>`; }
  if (s.error) return `<div class="stack"><div class="card"><p style="margin:0">${esc(t(s.error))}</p></div></div>`;
  const signed = s.signedIn, on = s.cloudOn === true;
  const where = s.spots.map((p) => `<div class="set-row"><div class="k"><b class="path">${esc(p.path)}</b><span>${esc(t(FROM[p.from] || ''))}${p.exists ? '' : ' · ' + esc(t('Not there yet'))}</span></div><span class="acts">${p.from === 'found' ? `<button type="button" class="btn sm" data-act="saves-forget" data-id="${esc(id)}">${esc(t('Not right?'))}</button>` : ''}${p.from === 'player' ? `<button type="button" class="btn sm" data-act="saves-place-reset" data-id="${esc(id)}">${esc(t('Use the Usual Place'))}</button>` : ''}<button type="button" class="btn sm" data-act="saves-place" data-id="${esc(id)}">${esc(t('Change'))}</button></span></div>`).join('');
  const cloudCard = !signed
    ? `<div class="card acc-cloud"><div class="acc-hero">${CLOUD_ICON}<div><b>${esc(t('Cloud saves'))}</b><span>${esc(t('Sign in to back this game\'s saves up to your account and play on with them on any PC.'))}</span></div></div><div class="acts grid2 acc-acts"><button type="button" class="btn" data-act="acc-create">${esc(t('Create an account'))}</button><button type="button" class="btn main" data-act="acc-signin">${esc(t('Sign in'))}</button></div></div>`
    : `<div class="card"><div class="set-row"><div class="k"><b>${esc(t('Cloud saves'))}</b><span>${esc(on ? (s.offline ? t('On. The website can\'t be reached right now; copies go up when it can.') : s.pending ? t('On. The newest copy goes up next time the launcher can reach the website.') : t('On. After you play, its saves go to your account.')) : t('Off. Its saves only stay on this PC.'))}</span></div><button type="button" class="switch" role="switch" aria-checked="${on}" aria-label="${esc(t('Cloud saves'))}" data-act="saves-cloud" data-id="${esc(id)}"></button></div>
      ${on && Array.isArray(s.cloud) ? (s.cloud.length ? s.cloud.map((c) => copyRow(id, c, 'v', c.v, c.hash === s.synced)).join('') : `<p class="set-empty">${esc(t('Nothing in the cloud yet. It goes up after you play.'))}</p>`) : ''}</div>`;
  const local = s.local.length ? s.local.map((c) => copyRow(id, c, 'name', c.name, false)).join('') : `<p class="set-empty">${esc(g.last_played ? t('No saves found yet. If it saves somewhere unusual, its developer can say where in recomplauncher.json.') : t('Play it once and its saves are copied here after.'))}</p>`;
  return `<div class="stack">${cloudCard}
    <div class="card"><div class="card-h"><h3>${esc(t('On this PC'))}</h3><span class="acts"><button type="button" class="btn sm" data-act="saves-open" data-id="${esc(id)}">${esc(t('Open the folder'))}</button><button type="button" class="btn sm main" data-act="saves-now" data-id="${esc(id)}"${s.busy ? ' disabled' : ''}>${esc(t('Back up now'))}</button></span></div>${local}</div>
    <div class="card"><h3 style="margin-bottom:6px">${esc(t('Where it saves'))}</h3>${where}</div></div>`;
}
function copyRow(id, c, k, v, current) {
  const when = c.when || c.made;
  return `<div class="set-row"><div class="k"><b>${esc(whenText(when))}${current ? ` <em class="acc-now">${esc(t('Same as this PC'))}</em>` : ''}</b><span>${esc([c.device, tn(c.files || 0, '1 file', '{n} files'), fmtB(c.size)].filter((x, i) => x && !(i === 1 && !c.files)).join(' · '))}</span></div><button type="button" class="btn sm" data-act="saves-restore" data-id="${esc(id)}" data-${k}="${esc(v)}">${esc(t('Put back'))}</button></div>`;
}

const savesChanged = (id) => { delete S.saves[id]; if (S.view === 'game' && S.arg === id && S.tab === 'saves') loadSaves(id); };

// ---------- the Saves page: every game's saves in one place ----------
const CARD_ICON = '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M7 3h10a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z"/><path d="M9 3v3.5M12 3v3.5M15 3v3.5M8.5 12h7v5h-7z"/></svg>';
let savesAllBusy = null;
async function loadSavesAll() {
  if (savesAllBusy) return savesAllBusy;
  savesAllBusy = (async () => {
    try { S.savesAll = await invoke('saves_overview'); } catch (e) { S.savesAll = { error: String(e && e.message ? e.message : e), games: [] }; }
    savesAllBusy = null;
    if (S.view === 'saves') render(true);
  })();
  return savesAllBusy;
}
// A row of blocks that fill up, like the space on a memory card.
function blocks(frac, n = 32) {
  const on = frac > 0 ? Math.max(1, Math.round(Math.min(1, frac) * n)) : 0;
  return `<div class="blocks" aria-hidden="true">${Array.from({ length: n }, (_, i) => `<i${i < on ? ' class="on"' : ''}></i>`).join('')}</div>`;
}
function pageSaves() {
  const o = S.savesAll;
  if (!o) { loadSavesAll(); return `<div class="loading">${esc(t('Loading…'))}</div>`; }
  const acc = S.account || {};
  const games = (o.games || []).map((x) => ({ ...x, g: S.library[x.id] || {}, a: byId(x.id) })).filter((x) => x.g.name || x.a);
  const withSaves = games.filter((x) => x.copies).sort((a, b) => (b.newest?.when || 0) - (a.newest?.when || 0));
  const none = games.filter((x) => !x.copies).sort((a, b) => (a.g.name || '').localeCompare(b.g.name || ''));
  const copies = withSaves.reduce((n, x) => n + x.copies, 0), size = withSaves.reduce((n, x) => n + x.size, 0);
  const cloudGames = games.filter((x) => x.cloudOn === true).length;
  const used = acc.used || 0, quota = acc.quota || 150 * 1024 * 1024;
  const pcCard = `<div class="mc"><div class="mc-top"><span class="mc-ico">${CARD_ICON}</span><div><b>${esc(t('On this PC'))}</b><span>${esc(tn(withSaves.length, '1 game with saves', '{n} games with saves'))}</span></div><strong>${esc(fmtB(size) || '0 KB')}</strong></div>
      ${blocks(withSaves.length ? withSaves.length / Math.max(games.length, 1) : 0)}
      <p>${esc(tn(copies, '1 copy kept', '{n} copies kept'))} · ${esc(t('The newest ten of each game'))}</p></div>`;
  const cloudCard = acc.signedIn
    ? `<div class="mc cloud"><div class="mc-top"><span class="mc-ico">${CLOUD_ICON}</span><div><b>${esc(t('Cloud saves'))}</b><span>${esc(tn(cloudGames, '1 game backs up to your account', '{n} games back up to your account'))}</span></div><strong>${esc(fmtB(used) || '0 KB')}</strong></div>
      ${blocks(used / quota)}
      <p>${esc(tf('{used} of {quota} used', { used: fmtB(used) || '0 KB', quota: fmtB(quota) }))}</p></div>`
    : `<div class="mc cloud off"><div class="mc-top"><span class="mc-ico">${CLOUD_ICON}</span><div><b>${esc(t('Cloud saves'))}</b><span>${esc(t('Sign in to keep your saves on any PC.'))}</span></div></div>
      <div class="acts grid2"><button type="button" class="btn" data-act="acc-create">${esc(t('Create an account'))}</button><button type="button" class="btn main" data-act="acc-signin">${esc(t('Sign in'))}</button></div></div>`;
  const slot = (x, i) => {
    const a = x.a || { id: x.id, name: x.g.name }, n = x.newest;
    const cloud = x.cloudOn === true ? (x.pending ? `<span class="tag wait">${esc(t('Going up'))}</span>` : `<span class="tag on">${esc(t('In the cloud'))}</span>`) : `<span class="tag">${esc(t('This PC'))}</span>`;
    return `<div class="slot" style="--i:${i}"><button type="button" class="slot-main" data-act="saves-game" data-id="${esc(x.id)}"><span class="slot-art">${a.cover ? pic(a.cover) : ''}</span>
        <span class="slot-txt"><b>${esc(a.name)}</b><span title="${esc(whenText(n.when))}">${esc(tf('Last saved {when}', { when: ago(n.when) }))}${n.device ? ' · ' + esc(n.device) : ''}</span></span>
        <span class="slot-meta">${cloud}<span class="slot-n">${esc(tn(x.copies, '1 copy', '{n} copies'))}<small>${esc(fmtB(x.size))}</small></span></span></button>
      <button type="button" class="icon-btn" data-act="saves-now" data-id="${esc(x.id)}" title="${esc(t('Back up now'))}" aria-label="${esc(t('Back up now'))} · ${esc(a.name)}"${S.running.has(x.id) ? ' disabled' : ''}><svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 20V9m0 0-4.5 4.5M12 9l4.5 4.5M5 4h14"/></svg></button></div>`;
  };
  const empty = !games.length
    ? `<div class="empty">${CARD_ICON.replace('width="20" height="20"', 'width="44" height="44"')}<h2>${esc(t('No games yet'))}</h2><p>${esc(t('Install a game and play it once. Its saves are copied here after every session.'))}</p></div>`
    : '';
  return `<div class="page saves-page"><div class="head"><div><h1>${esc(t('Saves'))}</h1><p>${esc(t('After every session the launcher keeps a copy of each game\'s saves. Pick a game to put an older one back.'))}</p></div></div>
    ${o.error ? `<div class="card"><p style="margin:0">${esc(t(o.error))}</p></div>` : ''}
    <div class="mcs">${pcCard}${cloudCard}</div>
    ${withSaves.length ? `<section class="shelf"><div class="shelf-h"><h2>${esc(t('Games'))}</h2></div><div class="slots">${withSaves.map(slot).join('')}</div></section>` : ''}
    ${none.length ? `<section class="shelf"><div class="shelf-h"><h2>${esc(t('No saves yet'))}</h2><span class="muted">${esc(t('Play them once and their saves show up here.'))}</span></div><div class="slots dim">${none.map((x) => { const a = x.a || { id: x.id, name: x.g.name }; return `<button type="button" class="slot-main mini" data-act="saves-game" data-id="${esc(x.id)}"><span class="slot-art">${a.cover ? pic(a.cover) : ''}</span><span class="slot-txt"><b>${esc(a.name)}</b><span>${esc(x.found ? t('Saves found, copied after you play') : t('Not played yet'))}</span></span></button>`; }).join('')}</div></section>` : ''}
    ${empty}</div>`;
}
window.pageSaves = pageSaves;
window.loadSavesAll = loadSavesAll;

// ---------- before Play ----------
// Asks once per game, while signed in, whether its saves go to the cloud; then brings in a newer
// save from another PC, or asks which to keep. false: don't start it.
async function ask(title, text, buttons) {
  if (inTV() && window.tvAsk) {
    const i = await window.tvAsk(title, buttons.map((b) => ({ label: b.label, sub: b.sub, icon: b.icon })));
    return i == null || i < 0 ? null : buttons[i].v;
  }
  return new Promise((res) => {
    let picked = null;
    const box = modal(`<h2>${esc(title)}</h2>${text}<div class="acts grid2">${buttons.map((b, i) => `<button type="button" class="btn${b.main ? ' main' : ''}" data-x="b${i}">${esc(b.label)}</button>`).join('')}</div>`, (x, close) => {
      const i = Number(String(x.dataset.x).slice(1));
      if (!isNaN(i) && buttons[i]) { picked = buttons[i].v; close(); }
    }, title);
    new MutationObserver((m, o) => { if (!box.isConnected) { o.disconnect(); res(picked); } }).observe(document.body, { childList: true });
  });
}
async function beforePlay(id) {
  const g = S.library[id];
  if (!g || !(S.account && S.account.signedIn)) return true;
  if (g.cloud == null) {
    const v = await ask(tf('Cloud saves for {name}?', { name: g.name }), `<p>${esc(t('After you play, its saves go to your account, so they\'re safe and ready on any PC. You can change this on its Saves tab.'))}</p>`,
      [{ label: t('Not now'), sub: t('Its saves stay on this PC'), icon: 'chevL', v: 'no' }, { label: t('Turn on'), sub: t('Back them up to your account'), icon: 'check', v: 'yes', main: true }]);
    if (v == null) return false;
    try { S.library[id] = await invoke('saves_cloud', { id, on: v === 'yes' }); } catch (e) { fail(e); }
    savesChanged(id);
    if (v !== 'yes') return true;
  }
  if (S.library[id].cloud !== true) return true;
  let r;
  try { r = await invoke('saves_check', { id }); } catch (e) { fail(e); return true; }
  if (r.action === 'restored') toast(tf('Your newer save from {device} is in place.', { device: r.cloud.device || t('another PC') }));
  else if (r.action === 'offline') toast(t('Cloud saves can\'t be reached right now. Playing with this PC\'s save.'), true);
  else if (r.action === 'conflict') {
    const L = r.local, C = r.cloud;
    const card = (h, when, dev, played) => `<div class="pick-card"><b>${esc(h)}</b><span>${esc(whenText(when))}</span><span>${esc(dev)}</span><span>${esc(tf('Played {time} in all', { time: fmtTime(played) }))}</span></div>`;
    const v = await ask(t('Which save do you want?'), `<p>${esc(tf('{name} was played on this PC and on another one since they last matched.', { name: g.name }))}</p><div class="pick-two">${card(t('This PC'), L.when, L.device, L.played)}${card(t('Cloud'), C.made, C.device, C.played)}</div><p class="muted" style="font-size:13px">${esc(t('The other one is kept as a copy on its Saves tab, so nothing is lost.'))}</p>`,
      [{ label: t('Keep this PC\'s'), sub: whenText(L.when), icon: 'desk', v: 'local' }, { label: t('Use the cloud one'), sub: `${whenText(C.made)} · ${C.device}`, icon: 'check', v: 'cloud', main: true }]);
    if (v == null) return false;
    try { await invoke('saves_pick', { id, keep: v }); } catch (e) { fail(e); return false; }
    if (v === 'cloud') toast(t('The cloud save is in place.'));
  }
  savesChanged(id);
  return true;
}

// ---------- actions ----------
window.accountAct = async (act, el) => {
  const id = el.dataset.id;
  switch (act) {
    case 'acc-signin': return signIn(false);
    case 'acc-create': return signIn(true);
    case 'acc-signout': return signOut();
    case 'acc-retry': el.disabled = true; await loadAccount(true); if (S.account && !S.account.offline) toast(t('Connected again.')); return render(true);
    case 'acc-btn': if (S.account && S.account.signedIn) { S.setTab = 'account'; return go('settings'); } return signIn(false);
    case 'saves-cloud': {
      const on = el.getAttribute('aria-checked') !== 'true';
      try { S.library[id] = await invoke('saves_cloud', { id, on }); } catch (e) { return fail(e); }
      toast(on ? t('Cloud saves are on for it.') : t('Cloud saves are off for it. What\'s in the cloud stays there.'));
      return loadSaves(id);
    }
    case 'saves-now': {
      el.disabled = true;
      try { const c = await invoke('saves_backup', { id }); toast(c ? t('Backed up.') : t('Nothing changed since the last copy.')); } catch (e) { fail(e); }
      if (S.view === 'saves') { el.disabled = false; return loadSavesAll(); }
      return loadSaves(id);
    }
    case 'saves-game': S.nextTab = 'saves'; return go('game', id, el.querySelector('.slot-art'));
    case 'saves-open': return invoke('saves_open', { id }).catch(fail);
    case 'saves-place': {
      // The player knows better (a game that saves somewhere else): their folder from now on.
      const p = await pickFile({ title: t('The folder where the game keeps its saves'), directory: true }).catch(fail);
      if (!p) return;
      try { S.library[id] = await invoke('saves_set_place', { id, path: typeof p === 'string' ? p : p.path }); } catch (e) { return fail(e); }
      toast(t('Saved. Backups and cloud saves use that folder now.'));
      return loadSaves(id);
    }
    case 'saves-place-reset': try { S.library[id] = await invoke('saves_set_place', { id, path: null }); } catch (e) { return fail(e); } return loadSaves(id);
    case 'saves-forget': try { S.library[id] = await invoke('saves_forget_found', { id }); } catch (e) { return fail(e); } return loadSaves(id);
    case 'saves-restore': {
      const v = await ask(t('Put this save back?'), `<p>${esc(t('The game goes back to this save. What\'s there now is kept as a copy first, so you can switch back.'))}</p>`,
        [{ label: t('Cancel'), v: null }, { label: t('Put it back'), v: 'yes', main: true }]);
      if (v !== 'yes') return;
      try { await invoke('saves_restore', { id, name: el.dataset.name || null, v: el.dataset.v || null }); toast(t('That save is back in place.')); } catch (e) { fail(e); }
      return loadSaves(id);
    }
  }
  return null;
};

T.event.listen('account', ({ payload }) => { if (payload && payload.signedIn === false) { S.account = { signedIn: false }; S.saves = {}; renderAccountBits(); } else loadAccount(true); });
T.event.listen('saves', ({ payload }) => { if (S.view === 'saves') loadSavesAll(); if (payload && payload.id) { delete S.saves[payload.id]; if (S.view === 'game' && S.arg === payload.id && S.tab === 'saves') loadSaves(payload.id); } });

// Who's signed in: what's remembered first, then the website's answer.
loadAccount(false).then(() => loadAccount(true));
