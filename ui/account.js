'use strict';
// The RecompStore account and cloud saves: signing in (browser or phone), Settings > Account, the
// sidebar's account button, a game's Saves tab, cloud saves for every game (asked once), and what
// happens before Play (a newer save from another PC? which save to keep?). Works the same in Big
// Picture mode, with its own sheets. Uses app.js's helpers and state (S).

const ACC_ICON = '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="8.5" r="3.6"/><path d="M4.8 20a7.2 7.2 0 0 1 14.4 0"/></svg>';
const CLOUD_ICON = '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M7 18a4.5 4.5 0 0 1-.6-9 6 6 0 0 1 11.5 1.6A3.8 3.8 0 0 1 17.5 18z"/></svg>';
const SITE = 'https://recomplauncher.com';
S.account = { signedIn: false };
S.saves = {};
// The signed-in player's reports (Settings > Account > Your Reports), with developers' answers.
S.myReports = null;

const whenText = (ts) => (ts ? new Date(ts * 1000).toLocaleString(curLocale(), timeOpts({ month: 'short', day: 'numeric', year: 'numeric', hour: 'numeric', minute: '2-digit' })) : '');
const avatar = (u, size = 28) => (u && u.avatar ? `<img class="acc-pic" src="${esc(u.avatar)}" alt="" width="${size}" height="${size}">` : `<span class="acc-pic none" style="width:${size}px;height:${size}px">${esc(((u && u.username) || '?').slice(0, 1).toUpperCase())}</span>`);
const inTV = () => document.body.classList.contains('tv-on');

async function loadAccount(fresh) {
  const was = !!(S.account && S.account.signedIn);
  try { S.account = await invoke('account_state', { fresh: !!fresh }); } catch (e) { S.account = S.account || { signedIn: false }; }
  // Signed in (or out) just now: what each game's Saves tab knew is out of date.
  if (was !== !!S.account.signedIn) { S.saves = {}; S.savesAll = null; S.myReports = null; forgetSaveStatus(); }
  renderAccountBits();
  if (S.account.signedIn && !S.myReports && !S.account.offline) loadMyReports();
  // Notify Me: the games they're waiting on (none when signed out).
  if (window.afterSignIn && !S.account.offline) window.afterSignIn();
  // The saved list, kept with the account.
  if (S.account.signedIn && !S.account.offline && typeof libSync === 'function') libSync();
  // Playtime over all the account's PCs (or back to this PC's own when signed out).
  if (typeof playSync === 'function') playSync().then((changed) => changed && render(true));
  // Sponsor spots for this account (none when signed out).
  if (typeof sponsSync === 'function') sponsSync();
  // Signed in and not asked yet: cloud saves for every game, or only the ones they pick.
  askCloudAll();
}

// ---------- your reports ----------
async function loadMyReports() {
  if (!(S.account && S.account.signedIn)) { S.myReports = null; return renderAccountBits(); }
  if (S.myReports && S.myReports.loading) return;
  const had = (S.myReports && S.myReports.list) || null;
  S.myReports = { list: had, loading: true };
  try { const r = await invoke('my_reports'); S.myReports = { list: (r && r.reports) || [] }; } catch (e) { S.myReports = { list: had, error: String(e && e.message ? e.message : e) }; }
  renderAccountBits();
}
window.loadMyReports = loadMyReports;
// A developer answered and the player hasn't looked yet: the newest developer answer on each report, against what was seen.
const repSeen = () => (S.settings.ui || {}).reports_seen || {};
const lastDevReply = (r) => Math.max(0, ...(r.replies || []).filter((x) => x.dev).map((x) => x.id || 0));
const reportsUnread = () => !!(S.myReports && S.myReports.list && S.myReports.list.some((r) => lastDevReply(r) > (repSeen()[r.id] || 0)));
function markReportsSeen() {
  const list = (S.myReports && S.myReports.list) || [];
  const seen = Object.assign({}, repSeen());
  let changed = false;
  for (const r of list) { const d = lastDevReply(r); if (d > (seen[r.id] || 0)) { seen[r.id] = d; changed = true; } }
  if (changed) saveUi({ reports_seen: seen }, true).then(() => { const b = $('#acc-btn .acc-dot'); if (b) b.remove(); });
}
setInterval(() => { if (S.account && S.account.signedIn && !document.hidden) loadMyReports(); }, 30 * 60 * 1000);
function renderAccountBits() {
  const b = $('#acc-btn');
  if (b) {
    const a = S.account || {};
    b.innerHTML = a.signedIn ? `${avatar(a.user, 22)}<span class="lbl">${esc((a.user && a.user.username) || t('Account'))}</span>${reportsUnread() ? `<i class="acc-dot" title="${esc(t('A developer answered your report'))}"></i>` : ''}` : `${ACC_ICON}<span class="lbl">${esc(t('Sign in'))}</span>`;
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
  if (window.notifyForget) window.notifyForget();
  invoke('account_cancel').catch(() => {});
}
// Desktop: the browser opens on the page that says yes, and the code is shown in case it's needed
// on another device. Big Picture mode: the code and a QR code for a phone.
async function signIn(create) {
  let s;
  try { s = await invoke('account_start', { create: !!create }); } catch (e) { if (window.notifyForget) window.notifyForget(); return fail(e); }
  const tv = inTV() && window.tvSignIn;
  let box, setStatus;
  const done = (user) => {
    clearTimeout(SIGN.timer); SIGN.timer = null;
    if (SIGN.sheet) { SIGN.sheet.close(true); SIGN.sheet = null; }
    if (box) { box.dataset.ok = '1'; box.remove(); }
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
      if (x.dataset.x === 'again') { box.dataset.ok = '1'; box.remove(); signIn(create); }
    }, t('Sign in'));
    // Closed some other way (Cancel, Esc, clicking outside): stop waiting.
    new MutationObserver((m, o) => { if (!box.isConnected) { o.disconnect(); if (SIGN.timer) stopSignIn(); else if (!box.dataset.ok && window.notifyForget) window.notifyForget(); } }).observe(document.body, { childList: true });
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
  forgetSaveStatus();
  // The next sign-in merges this PC's saved list with that account's again.
  saveUi({ lib_synced: null, lib_queue: [] }, true).catch(() => {});
  toast(t('Signed out. Your saves stay on this PC.'));
  renderAccountBits();
  if (window.afterSignIn) window.afterSignIn();
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
  // Back Up Every Game says what it does now, like a game's own switch in its Manage tab.
  const all = (S.settings.ui || {}).cloud_all === true;
  const allNow = all ? t('On. Every game\'s saves go to your account after you play. Turning it off changes every game.') : t('Off. Only the games you turn on in their Manage tab go to your account. Turning it on changes every game.');
  return group(t('Account'), `<div class="acc-me">${avatar(a.user, 48)}<div><b>${esc(a.user.username)}</b><span>${esc(a.offline ? t('Signed in on this PC. The website can\'t be reached right now.') : t('Signed in on this PC'))}${a.offline && a.why ? `<small class="muted" style="display:block">${esc(a.why)}</small>` : ''}</span></div>${a.offline ? `<button type="button" class="btn sm" data-act="acc-retry">${esc(t('Try again'))}</button>` : ''}</div>
      <div class="acts grid2 acc-acts"><button type="button" class="btn" data-act="url" data-url="${SITE}/account">${esc(t('Your account'))}</button><button type="button" class="btn" data-act="acc-signout">${esc(t('Sign out'))}</button></div>`)
    + group(t('Cloud saves'), `<div class="acc-space"><div class="acc-bar"><i style="width:${pct}%"></i></div><span>${esc(tf('{used} of {quota} used', { used: fmtB(used) || '0 KB', quota: fmtB(quota) }))}</span></div>
      <p class="set-note" style="margin:12px 0 0">${esc(t('Turn them on or off for one game in its Manage tab. The newest five copies of each game are kept.'))}</p>`
      + row(t('Back Up Every Game'), allNow, `<button type="button" class="switch" role="switch" aria-checked="${all}" aria-label="${esc(t('Back Up Every Game'))}" data-act="acc-cloud-all"></button>`)
      + deviceNameRow())
    + myReportsGroup(group, row)
    + (window.betasGroup ? window.betasGroup(group, row) : '');
}
// The name this PC goes by next to its save copies and in the cloud conflict prompt. Empty: the
// computer's own name. Only copies made after a change carry the new name.
function deviceNameRow() {
  const name = (S.settings.ui || {}).device_name || '';
  const label = t('This PC\'s Name');
  const help = t('Shown next to the saves this PC makes, so you can tell your PCs apart. Leave it empty to use the computer\'s own name. Saves already made keep the name they had.');
  const field = `<input class="field sm" id="device-name" type="text" maxlength="40" autocomplete="off" spellcheck="false" value="${esc(name)}" placeholder="${esc(t('Computer name'))}" aria-label="${esc(label)}">`;
  return `<div class="set-row"><div class="k"><b>${esc(label)}</b><span>${esc(help)}</span></div><span class="acts">${field}<button type="button" class="btn sm" data-act="acc-device-save">${esc(t('Save'))}</button></span></div>`;
}
// Your Reports: what you said about each game, and what its developer answered. Looking at it
// counts as having seen the answers.
function myReportsGroup(group, row) {
  const m = S.myReports;
  if (!m || (!m.list && m.loading)) return group(t('Your Reports'), `<p class="muted" style="margin:0">${esc(t('Loading…'))}</p>`);
  if (!m.list) return group(t('Your Reports'), row(t('Your reports couldn\'t be loaded'), m.error || '', `<button type="button" class="btn sm" data-act="myrep-load">${esc(t('Try Again'))}</button>`));
  if (!m.list.length) return group(t('Your Reports'), row(t('No reports yet'), t('When you say how a game runs, it shows here with any answer from its developer.'), ''));
  setTimeout(markReportsSeen, 0);
  const seen = repSeen();
  const when = (w) => { const ts = Date.parse(w) / 1000; return ts ? ago(ts) : ''; };
  const item = (r) => {
    const fresh = lastDevReply(r) > (seen[r.id] || 0);
    const replies = (r.replies || []).map((x) => `<div class="rep-reply${x.dev ? ' dev' : ''}"><div class="who">${x.dev ? `<b>${esc(x.name || t('Developer'))}</b><span class="dev-badge">${esc(t('Developer'))}</span>` : `<b>${esc(t('You'))}</b>`}<small>${esc(when(x.when))}</small></div><p dir="auto">${esc(x.text)}</p></div>`).join('');
    const answer = (r.replies || []).some((x) => x.dev) ? `<div class="myrep-ans"><textarea class="field" rows="2" maxlength="2000" data-myrep="${r.id}" placeholder="${esc(t('Answer the developer'))}" aria-label="${esc(t('Answer the developer'))}"></textarea><button type="button" class="btn sm" data-act="myrep-send" data-id="${r.id}">${esc(t('Send'))}</button></div>` : '';
    return `<div class="myrep${fresh ? ' fresh' : ''}"><div class="myrep-h"><b>${esc(r.name || r.app)}</b><span class="rverdict v-${esc(r.verdict)}"><i aria-hidden="true"></i>${esc(t(RUN_WORD[r.verdict] || ''))}</span><small>${esc([r.version, when(r.when)].filter(Boolean).join(' · '))}</small></div>
      ${r.note ? `<p class="rep-note" dir="auto">${esc(r.note)}</p>` : ''}${replies ? `<div class="rep-replies">${replies}</div>` : ''}${answer}</div>`;
  };
  return group(t('Your Reports'), `<p class="set-note" style="margin:0 0 6px">${esc(t('What you said about each game, and what its developer answered.'))}</p>${m.list.slice(0, 30).map(item).join('')}`);
}

// ---------- a game's Saves tab ----------
async function loadSaves(id) {
  S.saves[id] = S.saves[id] && S.saves[id] !== 'loading' ? Object.assign(S.saves[id], { busy: true }) : 'loading';
  try { S.saves[id] = await invoke('saves_info', { id }); } catch (e) { S.saves[id] = { error: String(e && e.message ? e.message : e) }; }
  // The tab asked the cloud just now: the badge above Play says the same.
  if (S.saves[id] && !S.saves[id].error) S.saveNext[saveIdOf(id)] = S.saves[id].incoming || null;
  if (S.view === 'game' && S.arg === id && S.tab === 'saves') refreshGame();
  // Whatever just changed the tab (Sync Now, Back Up Now, the cloud switch) changed the badge too.
  loadSaveStatus(saveIdOf(id));
}
const FROM = { player: N_('You picked this folder'), developer: N_('Set by its developer'), catalog: N_('Set by RecompLauncher') };
// A folder its developer or RecompLauncher named is the right one: it's said in green, with a
// shield, so the player sees there's nothing to set up. One the player picked is said plainly.
const SET_ICON = '<svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3 5 6v5c0 4.4 3 8.3 7 9.5 4-1.2 7-5.1 7-9.5V6z"/><path d="m9 12 2.2 2.2L15.5 10"/></svg>';
const spotSet = (p) => p.from === 'developer' || p.from === 'catalog';
// Nobody (the player, its developer or RecompLauncher) has said where this game saves. Nothing is
// guessed: the player picks the folder, and until then nothing of it is backed up.
const SAVES_UNKNOWN = N_('The launcher doesn\'t know where this game saves');
const SAVES_UNKNOWN_HELP = N_('Nothing is backed up or sent to the cloud until you pick the folder where it keeps its saves.');
const SAVES_PICK = N_('Pick a Folder');
const savesUnknown = (s) => !!(s && Array.isArray(s.spots) && !s.spots.length);
window.savesUnknown = savesUnknown;
const SHARE_SAVES = N_('Share saves between versions');
const SHARE_SAVES_HELP = N_('Ask before copying a newer save from another installed version.');
const saveShareTitle = (id) => !subOf(id) && verGroup(byId(id))?.id;
const saveShareOn = (title) => ((S.settings.ui || {}).save_share_titles || []).includes(title);
async function toggleSaveShare(title) {
  const current = (S.settings.ui || {}).save_share_titles || [];
  await saveUi({ save_share_titles: current.includes(title) ? current.filter((x) => x !== title) : [...current, title] }, true);
}
// The tab, top to bottom: where its saves stand (with the one thing to do about it), every save
// in one list, its settings, and where it saves. A game nobody has said the save folder of leads
// with picking it instead, since nothing else works until then.
function savesTab(a) {
  const id = a.id, s = S.saves[id], g = S.library[id] || {};
  if (!s || s === 'loading') { if (!s) loadSaves(id); return `<div class="stack"><div class="card"><p class="muted" style="margin:0">${esc(t('Loading…'))}</p></div></div>`; }
  if (s.error) return `<div class="stack"><div class="card"><p style="margin:0">${esc(t(s.error))}</p></div></div>`;
  if (savesUnknown(s)) return `<div class="stack">${unknownCard(id)}${historyCard(id, s, g, true)}${savesSettingsCard(id)}</div>`;
  return `<div class="stack">${saveStatusCard(id, s)}${historyCard(id, s, g, false)}${savesSettingsCard(id)}${whereCard(id, s)}</div>`;
}

// Nobody has said where it saves: nothing is backed up or uploaded, even with cloud saves on, so
// it's red like a problem, and Pick a Folder is the way out.
function unknownCard(id) {
  return `<div class="card svs sb-bad" role="status"><div class="svs-row"><span class="svs-ico">${badgeIcon('alert', 30)}</span><div class="k"><b>${esc(t(SAVES_UNKNOWN))}</b><span>${esc(t(SAVES_UNKNOWN_HELP))}</span></div><span class="acts"><button type="button" class="btn sm main" data-act="saves-place" data-id="${esc(id)}">${esc(t(SAVES_PICK))}</button></span></div></div>`;
}

// A folder its developer set that's too big to look through: the player can't change it, so
// the way out is telling the developer.
const FOLDER_SET_TOO_BIG = N_('Its save folder is too big to look through, so cloud saves are paused for this game. Your saves stay on this PC. Its developer can fix where it saves: report it to them.');

// Where its saves stand, in the same words and colours as the line above Play, and the one thing
// to do about it.
function saveStatusCard(id, s) {
  const b = saveBadgeFor(id);
  if (!b) return '';
  const sid = saveIdOf(id);
  const st = S.saveStat[sid] || {};
  const phase = S.savePhase[sid], running = S.running.has(parentId(sid));
  const next = incomingSave(S.saveNext[sid], st, phase, running);
  const picked = s.spots.some((p) => p.from === 'player');
  const note = b.note ? ` <small>· ${esc(b.note)}</small>` : '';
  const rows = b.rows.map(([k, v]) => `${k}: ${v}`).join(' · ');
  const text = statusCardText(st, b, picked);
  const offline = st.problem && st.problem.kind === 'offline' && s.status;
  const help = offline ? `<p class="set-note">${esc(t('Still no answer. Check whether the website is having trouble:'))} <button type="button" class="linkish" data-act="url" data-url="${esc(s.status)}">${esc(s.status.replace(/^https:\/\//, ''))}</button></p>` : '';
  const space = s.signedIn && s.cloudOn === true ? cloudSpace() : '';
  const step = next ? INCOMING_STEP : saveNextStep(st, phase, running, picked);
  const url = step && step.url ? ` data-url="${esc(step.url)}"` : '';
  const btn = step ? `<button type="button" class="btn sm main" data-act="${step.act}" data-id="${esc(id)}"${url}${s.busy ? ' disabled' : ''}>${esc(t(step.label))}</button>` : '';
  return `<div class="card svs sb-${b.tone}" role="status"><div class="svs-row"><span class="svs-ico">${badgeIcon(b.mark, 30)}</span>
    <div class="k"><b>${esc(b.label)}${note}</b>${text ? `<span>${esc(text)}</span>` : ''}${rows ? `<small>${esc(rows)}</small>` : ''}</div>
    <span class="acts">${btn}</span></div>${help}${space}</div>`;
}

// The cloud save another PC made that Play puts in place by itself (S.saveNext), when nothing else
// is going on, or null. This PC's own notes still call that "Up to date", and a Restore button on
// the newest save reads like a step to do on every PC: the status and its row say it syncs when
// you press Play instead.
function incomingSave(next, st, phase, running) {
  if (!next || phase || running || st.known === false) return null;
  if (st.state !== 'up-to-date' && st.state !== 'none') return null;
  return next;
}
// Sync Now gets it before playing, the same button as when it's up to date.
const INCOMING_STEP = { act: 'saves-sync', label: N_('Sync Now') };
const SYNCS_ON_PLAY = N_('Syncs when you press Play');

function incomingBadge(c) {
  const label = c.device ? tf('Newer save from {device}', { device: c.device }) : t('Newer save in the cloud');
  const text = tf('Saved {time}. What\'s there now is kept as a backup first.', { time: sinceText(c.made) });
  return { tone: 'cy', mark: 'next', label, note: t('syncs when you press Play'), head: t('A newer save is in the cloud'), text, rows: [] };
}

// The status's line on the tab: the badge's, except where the badge points somewhere else.
function statusCardText(st, b, picked) {
  // On the tab the way to pick is the button beside it, not "Play, or Sync Now".
  if (st.state === 'conflict') return t('This PC and the cloud both changed since they last synced. Pick the one to keep; the other stays as a copy on this PC.');
  if (st.state === 'off') return t('The launcher still keeps a copy after every session. Turn cloud saves on to have it on any PC.');
  const kind = st.state === 'problem' && st.problem && st.problem.kind;
  if (kind === 'too-big-folder' && !picked) return t(FOLDER_SET_TOO_BIG);
  return b.text;
}

// What the player can do about a status, so none leaves them stuck: a button ({ act, label }),
// or null when it sorts itself out (a sync running, the game being played, an upload's next try,
// cloud saves paused for everyone, nothing played yet). `picked`: the player chose the folder.
function saveNextStep(st, phase, running, picked) {
  if (phase || running) return null;
  if (st.known === false) return { act: 'saves-place', label: N_('Pick a Folder') };
  if (st.state === 'signed-out') return { act: 'acc-signin', label: N_('Sign in') };
  // saves-cloud turns them on when the button has no switch state to read.
  if (st.state === 'off') return { act: 'saves-cloud', label: N_('Turn On') };
  if (st.state === 'conflict') return { act: 'saves-sync', label: N_('Choose a Save') };
  if (st.state === 'problem') return problemStep((st.problem && st.problem.kind) || 'error', picked);
  if (st.state === 'waiting' || st.state === 'up-to-date') return { act: 'saves-sync', label: N_('Sync Now') };
  return null;
}

function problemStep(kind, picked) {
  if (kind === 'too-big-folder') return picked ? { act: 'saves-place', label: N_('Pick a Folder') } : { act: 'report', label: N_('Report a Problem') };
  // A save over the upload limit is the game's to make smaller.
  if (kind === 'too-big-copy') return { act: 'report', label: N_('Report a Problem') };
  // The site paused cloud saves; they start again by themselves.
  if (kind === 'paused') return null;
  // The newest save of every game is kept, so room comes from another game's cloud saves: the
  // account page lists them all.
  if (kind === 'full') return { act: 'url', label: N_('Free Up Space'), url: `${SITE}/account` };
  return { act: 'saves-sync', label: N_('Try Again') };
}

// How full the account's cloud saves are, the same bar as Settings → Account.
function cloudSpace() {
  const acc = S.account || {};
  const used = acc.used || 0, quota = acc.quota || 150 * 1024 * 1024;
  const pct = Math.min(100, Math.round((used / quota) * 100));
  return `<div class="acc-space svs-space"><div class="acc-bar"><i style="width:${pct}%"></i></div><span>${esc(tf('Cloud space: {used} of {quota} used', { used: fmtB(used) || '0 KB', quota: fmtB(quota) }))}</span></div>`;
}

// Every save in one list, newest first. A copy kept on this PC and the same save in the cloud
// (the same checksum) are one row, marked as being in both places. The one in the game's folder
// now is Current.
function saveHistory(s) {
  const rows = [];
  const byHash = new Map();
  for (const c of s.local || []) {
    const row = { when: c.when, device: c.device, played: c.played, files: c.files, size: c.size, hash: c.hash, name: c.name, v: null, current: false };
    rows.push(row);
    if (c.hash && !byHash.has(c.hash)) byHash.set(c.hash, row);
  }
  for (const c of Array.isArray(s.cloud) ? s.cloud : []) {
    const same = c.hash && byHash.get(c.hash);
    if (same) { same.v = same.v || c.v; continue; }
    rows.push({ when: c.made, device: c.device, played: c.played, files: 0, size: c.size, hash: c.hash, name: null, v: c.v, current: false });
  }
  rows.sort((p, q) => q.when - p.when);
  const now = s.current && rows.find((r) => r.hash === s.current);
  if (now) now.current = true;
  // The cloud save Play brings in by itself is marked so, not offered as a Restore.
  const next = s.incoming ? s.incoming.v : null;
  for (const r of rows) r.incoming = !r.current && !!next && r.v === next;
  // Any cloud save but the newest can be deleted (saves_cloud_delete says why not that one).
  const newestCloud = Array.isArray(s.cloud) && s.cloud.length ? s.cloud.reduce((p, q) => (q.made > p.made ? q : p)).v : '';
  for (const r of rows) r.deletable = !!r.v && r.v !== newestCloud;
  return rows;
}

// Where a save is kept, and whether another PC made it.
function historyMarks(r, s) {
  const out = [];
  if (r.name) out.push({ k: 'pc', label: t('On this PC') });
  if (r.v) out.push({ k: 'cloud', label: t('In the cloud') });
  if (r.device && s.here && r.device !== s.here) out.push({ k: 'other', label: tf('From {device}', { device: r.device }) });
  return out;
}
const MARK_ICONS = {
  pc: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="4" width="18" height="12" rx="2"/><path d="M8 20h8M12 16v4"/></svg>',
  cloud: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M7 18a4.5 4.5 0 0 1-.6-9 6 6 0 0 1 11.5 1.6A3.8 3.8 0 0 1 17.5 18z"/></svg>',
  other: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 6h14v9H5zM2 19h20"/></svg>',
};

const BIN_ICON = '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 7h16M10 11v6M14 11v6M5 7l1 12a2 2 0 0 0 2 2h8a2 2 0 0 0 2-2l1-12M9 7V4h6v3"/></svg>';
const CURRENT_ICON = '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m5 12.5 4.5 4.5L19 7.5"/></svg>';
const NEXT_ICON = '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 5v14m0 0-5.5-5.5M12 19l5.5-5.5"/></svg>';

// Details under a save: how long the game had been played then, its files and size.
function historyDetails(r) {
  const played = r.played ? tf('{time} played', { time: fmtTime(r.played) }) : '';
  const files = r.files ? tn(r.files, '1 file', '{n} files') : '';
  return [played, files, fmtB(r.size)].filter(Boolean).join(' · ');
}

function historyRow(id, r, s) {
  const marks = historyMarks(r, s).map((m) => `<span class="svm svm-${m.k}">${MARK_ICONS[m.k]}${esc(m.label)}</span>`).join('');
  // A copy on this PC is restored from here; one only in the cloud comes down first.
  const which = r.name ? `data-name="${esc(r.name)}"` : `data-v="${esc(r.v)}"`;
  const put = `<button type="button" class="btn sm" data-act="saves-restore" data-id="${esc(id)}" ${which}>${esc(t('Restore'))}</button>`;
  // The save in the game now has nothing to restore: its row is lit instead, and says so where
  // the others have their button.
  const cur = `<span class="svh-cur">${CURRENT_ICON}${esc(t('In the game now'))}</span>`;
  const next = `<span class="svh-next">${NEXT_ICON}${esc(t(SYNCS_ON_PLAY))}</span>`;
  const side = r.current ? cur : r.incoming ? next : put;
  // Deleting from the cloud is quieter than Restore: an icon, with its name on hover.
  const del = r.deletable ? `<button type="button" class="icon-btn svh-del" data-act="saves-cloud-delete" data-id="${esc(id)}" data-v="${esc(r.v)}" title="${esc(t('Delete from the cloud'))}" aria-label="${esc(t('Delete from the cloud'))}">${BIN_ICON}</button>` : '';
  return `<div class="set-row svh-row${r.current ? ' cur' : ''}"><div class="k"><b>${esc(whenText(r.when))}</b><span class="svh-marks">${marks}</span><span>${esc(historyDetails(r))}</span></div><span class="acts">${del}${side}</span></div>`;
}

// The newest few; the rest one click away, so a long history doesn't push the settings far down.
const HISTORY_SHOWN = 6;
S.savesMore = new Set();
// The newest few, and always down to the one in the game now: after an older save is restored,
// its row stays where its date puts it and the list reaches it.
function historyShown(id, rows) {
  if (S.savesMore.has(id)) return rows;
  const current = rows.findIndex((r) => r.current);
  return rows.slice(0, Math.max(HISTORY_SHOWN, current + 1));
}

// Show older saves, or, once they're all shown, fewer again: { more, label }, or null when the
// list is short. `more`: how many are hidden.
function historyToggle(id, rows, more) {
  if (more) return { more: true, label: tn(more, 'Show 1 older save', 'Show {n} older saves') };
  if (S.savesMore.has(id) && rows.length > HISTORY_SHOWN) return { more: false, label: t('Show fewer saves') };
  return null;
}

function historyCard(id, s, g, unknown) {
  const rows = saveHistory(s);
  const shown = historyShown(id, rows);
  const more = rows.length - shown.length;
  const backups = `<button type="button" class="btn sm" data-act="saves-open-backups" data-id="${esc(id)}" title="${esc(t('Where the launcher keeps its copies of them'))}">${esc(t('Open Backups'))}</button>`;
  const now = `<button type="button" class="btn sm main" data-act="saves-now" data-id="${esc(id)}"${s.busy ? ' disabled' : ''}>${esc(t('Back up now'))}</button>`;
  // Nothing to back up until a save folder is known; copies made before can still be opened. With
  // cloud saves on, Sync Now copies the save before it goes up, so Back Up Now is only for a game
  // whose saves stay on this PC.
  const syncs = s.signedIn && s.cloudOn === true;
  const acts = unknown ? (s.local.length ? backups : '') : syncs ? backups : backups + now;
  const emptyText = unknown ? t('No backups yet.') : g.last_played ? t('No saves found yet. If it saves somewhere unusual, its developer can say where in recomplauncher.json.') : t('Play it once and its saves are copied here after.');
  const list = rows.length ? shown.map((r) => historyRow(id, r, s)).join('') : `<p class="set-empty">${esc(emptyText)}</p>`;
  const offline = s.cloudOn === true && s.offline ? `<p class="set-note">${esc(t('The cloud can\'t be reached right now, so only the copies on this PC are listed.'))}</p>` : '';
  const toggle = historyToggle(id, rows, more);
  const toggleBtn = toggle ? `<button type="button" class="btn sm svh-more" data-act="${toggle.more ? 'saves-more' : 'saves-less'}" data-id="${esc(id)}">${esc(toggle.label)}</button>` : '';
  return `<div class="card"><div class="card-h"><h3>${esc(t('Save History'))}</h3><span class="acts">${acts}</span></div>${offline}${list}${toggleBtn}</div>`;
}

// Sharing saves between its versions, for a game with more than one. Cloud saves on or off is in
// the game's Manage section; here the status card only offers to turn them on.
function savesSettingsCard(id) {
  const shareTitle = saveShareTitle(id);
  if (!shareTitle) return '';
  return `<div class="card"><h3 style="margin-bottom:6px">${esc(t('Settings'))}</h3><div class="set-row"><div class="k"><b>${esc(t(SHARE_SAVES))}</b><span>${esc(t(SHARE_SAVES_HELP))}</span></div><button type="button" class="switch" role="switch" aria-checked="${saveShareOn(shareTitle)}" aria-label="${esc(t(SHARE_SAVES))}" data-act="saves-share" data-id="${esc(id)}"></button></div></div>`;
}

// Where the game keeps its saves. A folder its developer (or RecompLauncher) named is only shown,
// with a way to open it: saying where is what recomplauncher.json is for, so there's nothing to
// change. Changing it and more folders are for a folder the player picked, which only counts
// while nobody has named one (saves::spots).
function whereCard(id, s) {
  const picked = s.spots.filter((p) => p.from === 'player').length;
  const rows = s.spots.map((p) => spotRow(id, p, picked > 1)).join('');
  const add = picked ? `<div class="set-row"><div class="k"><b>${esc(t('Saves in more than one folder?'))}</b><span>${esc(t('For a launcher with several games in it, each saving in its own folder. Backups and cloud saves cover every folder.'))}</span></div><span class="acts"><button type="button" class="btn sm" data-act="saves-place-add" data-id="${esc(id)}">${esc(t('Add a Folder'))}</button></span></div>` : '';
  return `<div class="card"><h3 style="margin-bottom:6px">${esc(t('Where it saves'))}</h3>${rows}${add}</div>`;
}

function spotRow(id, p, many) {
  const from = esc(t(FROM[p.from] || ''));
  const notYet = p.exists ? '' : ` · ${esc(t('Not there yet'))}`;
  const said = spotSet(p) ? `<span class="svp-set${p.from === 'catalog' ? ' ours' : ''}">${SET_ICON}${from}</span>${notYet}` : `${from}${notYet}`;
  const open = p.exists ? `<button type="button" class="btn sm" data-act="saves-open" data-id="${esc(id)}" title="${esc(t('Where the game keeps its saves'))}">${esc(t('Open'))}</button>` : '';
  let acts = open;
  if (many) acts = `<button type="button" class="btn sm" data-act="saves-place-remove" data-id="${esc(id)}" data-t="${esc(p.template)}">${esc(t('Take Off the List'))}</button>`;
  else if (p.from === 'player') acts = `${open}<button type="button" class="btn sm" data-act="saves-place" data-id="${esc(id)}">${esc(t('Change'))}</button>`;
  return `<div class="set-row"><div class="k"><b class="path">${esc(p.path)}</b><span>${said}</span></div><span class="acts">${acts}</span></div>`;
}

const savesChanged = (id) => { delete S.saves[id]; if (S.view === 'game' && S.arg === id && S.tab === 'saves') loadSaves(id); };

// ---------- the cloud save status above Play ----------
// Where a game's saves stand, on its page above Play and above Big Picture's game buttons, so the
// player doesn't have to open the Saves tab to see whether the last save reached the cloud. Only
// a coloured line, not a button: what to do about it is on the Saves tab. Hovering it says more.
// saves_status answers from this PC alone (it never asks the website); the save-sync event says
// when something runs or ends, and the status is asked again then.
S.saveStat = {};
S.savePhase = {};
// A newer save another PC made, that Play brings in by itself (saves_peek, or the Saves tab's
// saves_info): the one thing the status needs the website for.
S.saveNext = {};
const statLoading = new Set();
const peekLoading = new Set();

// A game of several that keeps its own saves goes by its own id; any other by its recomp's.
function saveIdOf(id) {
  const s = subOf(id);
  const own = s && Array.isArray(s.game.saves) && s.game.saves.length;
  return own ? id : parentId(id);
}

async function loadSaveStatus(sid) {
  if (statLoading.has(sid)) return;
  statLoading.add(sid);
  try { S.saveStat[sid] = await invoke('saves_status', { id: sid }); } catch (e) { S.saveStat[sid] = null; }
  statLoading.delete(sid);
  paintSaveBadges(sid);
  refreshSavesTab(sid);
  const st = S.saveStat[sid];
  if (st && (st.state === 'up-to-date' || st.state === 'none')) peekSave(sid);
}

// Asks the website whether another PC saved after this one. The core asks at most every few
// minutes per game, and says nothing new while offline.
async function peekSave(sid) {
  if (peekLoading.has(sid)) return;
  peekLoading.add(sid);
  let next = null;
  try { next = await invoke('saves_peek', { id: sid }); } catch (e) { next = null; }
  peekLoading.delete(sid);
  const was = S.saveNext[sid] ? S.saveNext[sid].v : null;
  S.saveNext[sid] = next;
  if ((next ? next.v : null) === was) return;
  paintSaveBadges(sid);
  refreshSavesTab(sid);
}

// "12 min ago" for the last sync: ago() only goes down to "today", too coarse for "did that
// session's save go up?".
function sinceText(ts) {
  if (!ts) return '';
  const secs = Math.max(0, Date.now() / 1000 - ts);
  if (secs < 60) return t('just now');
  if (secs < 3600) return tn(Math.floor(secs / 60), '1 min ago', '{n} min ago');
  if (secs < 86400) return tn(Math.floor(secs / 3600), '1 hour ago', '{n} hours ago');
  return ago(ts);
}

// What a problem kept by the last sync looks like, by its kind. The backend's own words say why
// (in the tooltip); these say it short.
const PROBLEM_BADGE = {
  full: { label: N_('Can\'t upload'), note: N_('cloud full'), head: N_('Your cloud saves are full') },
  'too-big-copy': { label: N_('Too big for the cloud'), head: N_('This save is too big for the cloud') },
  'too-big-folder': { label: N_('Sync paused'), head: N_('Cloud saves are paused for this game') },
  paused: { label: N_('Cloud saves paused'), head: N_('Cloud saves are paused for now') },
  error: { label: N_('Can\'t upload'), head: N_('The last upload didn\'t go through') },
};

// While something runs (from the save-sync event): checking before Play, a save going up or one
// coming down.
const PHASE_BADGE = {
  check: { mark: 'check-run', label: N_('Checking cloud…'), head: N_('Looking for a newer save'), text: N_('Before the game starts, the launcher checks whether another PC saved after this one.') },
  up: { mark: 'up', label: N_('Uploading…'), head: N_('Sending your save'), text: N_('Your newest save is going to the cloud.') },
  down: { mark: 'down', label: N_('Getting cloud save…'), head: N_('Bringing in a newer save'), text: N_('Another PC saved after this one. Your current save is kept as a backup first.') },
  // The game just closed (or crashed, or the launcher is finishing a session it didn't see end):
  // its save is being copied, and sent when cloud saves are on.
  after: { mark: 'play', label: N_('Syncs after you play'), head: N_('Saving after you played'), text: N_('Its save is being copied, and sent to the cloud when cloud saves are on.') },
};

// What the status line shows for a game: its tone (ok, cy, warn, bad, dim), the mark inside the
// cloud, a label and a short note, and for the tooltip a heading, a line and rows ([label, value]).
// `phase` is what's running now, `running` whether the game is.
function saveBadgeInfo(st, phase, running) {
  // The copy after a session runs for every game; it's only shown for one that syncs. Signed out
  // or with cloud saves off, the line keeps saying so instead of a sync that isn't happening.
  const syncs = st.state !== 'signed-out' && st.state !== 'off' && st.known !== false;
  const p = syncs && PHASE_BADGE[phase];
  if (p) return { tone: 'cy', mark: p.mark, label: t(p.label), note: '', head: t(p.head), text: t(p.text), rows: [] };
  // Nobody has said where it saves: nothing is backed up or uploaded, whatever else is set. Many
  // games are like this until their folder is known, so it's grey here, not a red alarm on every
  // page; the Saves tab says it in red, with Pick a Folder.
  if (st.known === false) return { tone: 'dim', mark: 'off', label: t('Saves not backed up'), note: '', head: t('The launcher doesn\'t know where this game saves'), text: t('Nothing of it is backed up or sent to the cloud. Pick its save folder on its Saves tab.'), rows: [] };
  const synced = st.syncedAt ? [[t('Last synced'), whenText(st.syncedAt)], [t('Saved on'), st.syncedDevice || t('this PC')]] : [];
  const newest = st.newest ? [[t('Last backup'), `${whenText(st.newest.when)}${st.newest.device ? ' · ' + st.newest.device : ''}`]] : [];
  if (st.state === 'signed-out') return { tone: 'dim', mark: 'dash', label: t('Not signed in'), note: '', head: t('Keep this save on any PC'), text: t('Sign in to RecompStore and your saves go to the cloud after you play. It\'s free.'), rows: newest };
  if (st.state === 'off') return { tone: 'dim', mark: 'off', label: t('Cloud saves off'), note: '', head: t('Saves stay on this PC'), text: t('The launcher still keeps a copy after every session. Turn cloud saves on in its Saves tab.'), rows: newest };
  if (st.state === 'conflict') return { tone: 'warn', mark: 'conflict', label: t('Choose a save'), note: t('press Play to pick'), head: t('Both saves changed'), text: t('This PC and the cloud both changed since they last synced. Press Play, or Sync Now in its Saves tab, to pick the one to keep; the other stays as a backup on this PC.'), rows: synced };
  if (st.state === 'problem') return problemBadge(st, synced);
  if (running) return { tone: 'cy', mark: 'play', label: t('Syncs after you play'), note: '', head: t('Playing now'), text: t('Its save goes to the cloud when you close the game.'), rows: synced };
  if (st.state === 'waiting') return waitingBadge(st, synced);
  if (st.state === 'up-to-date') return { tone: 'ok', mark: 'check', label: t('Up to date'), note: sinceText(st.syncedAt), head: t('Your save is in the cloud'), text: t('This PC and the cloud have the same save.'), rows: synced };
  return { tone: 'dim', mark: 'dash', label: t('No saves yet'), note: '', head: t('Nothing to sync yet'), text: t('Play it once. Its saves are backed up after, and go to the cloud too.'), rows: [] };
}

function problemBadge(st, synced) {
  const kind = (st.problem && st.problem.kind) || 'error';
  const b = PROBLEM_BADGE[kind] || PROBLEM_BADGE.error;
  const text = st.problem && st.problem.text ? t(st.problem.text) : '';
  return { tone: 'bad', mark: 'alert', label: t(b.label), note: b.note ? t(b.note) : '', head: t(b.head), text, rows: synced };
}

// Saved here and not in the cloud yet: queued after a try that didn't get through (with when the
// next one is), or going up after the next session.
function waitingBadge(st, synced) {
  const queued = st.waiting > 0;
  const offline = st.problem && st.problem.kind === 'offline';
  const minutes = st.retryIn ? Math.max(1, Math.ceil(st.retryIn / 60)) : 0;
  const note = minutes ? tn(minutes, 'next try in 1 min', 'next try in {n} min') : offline ? t('offline') : '';
  const text = !queued ? t('It goes up after you play next, or now with Sync Now in its Saves tab.')
    : offline ? t('The website can\'t be reached right now. Your save is safe here and goes up by itself when it can.')
    : t('It goes up by itself on the next try.');
  const rows = queued ? [[t('Waiting'), tn(st.waiting, '1 copy', '{n} copies')], ...synced] : synced;
  return { tone: 'warn', mark: 'clock', label: t('Waiting to upload'), note, head: t('Saved on this PC, not in the cloud yet'), text, rows };
}

// The cloud, with a mark inside it for the state. Drawn the same in both modes.
const BADGE_MARKS = {
  check: '<path d="m9.4 13.4 2 2 3.4-3.6"/>',
  'check-run': '<path class="sb-spin" d="M14.6 13a2.6 2.6 0 1 1-1-2"/>',
  up: '<path class="sb-bob" d="M12 16.2v-5.4m0 0-2.3 2.3M12 10.8l2.3 2.3"/>',
  down: '<path class="sb-bob" d="M12 10.8v5.4m0 0-2.3-2.3M12 16.2l2.3-2.3"/>',
  // The same arrow, still: a save that comes down at the next Play, not one coming down now.
  next: '<path d="M12 10.8v5.4m0 0-2.3-2.3M12 16.2l2.3-2.3"/>',
  clock: '<path d="M12 11v2.4l1.6 1"/>',
  alert: '<path d="M12 10.6v2.8M12 15.7h.01"/>',
  conflict: '<path d="M10 10.6v5.2m-1.6-1.6L10 15.8l1.6-1.6M14 15.8v-5.2m-1.6 1.6L14 10.6l1.6 1.6"/>',
  off: '<path d="M4 4l16 16"/>',
  dash: '<path d="M10 13.4h4"/>',
  play: '<path d="M11 11v4.4l3.4-2.2z" fill="currentColor" stroke="none"/>',
};
const badgeIcon = (mark, size = 18) => `<svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M7 18a4.5 4.5 0 0 1-.6-9 6 6 0 0 1 11.5 1.6A3.8 3.8 0 0 1 17.5 18z"/>${BADGE_MARKS[mark] || ''}</svg>`;

// The tooltip: the heading, its line, then each row on its own line.
const badgeTip = (b) => [b.head, b.text, ...b.rows.map(([k, v]) => `${k}: ${v}`)].filter(Boolean).join('\n');

// The status for a game page id, or null while it's not known (or not installed).
function saveBadgeFor(id) {
  if (!S.library[parentId(id)]) return null;
  const sid = saveIdOf(id);
  const st = S.saveStat[sid];
  if (st === undefined) loadSaveStatus(sid);
  if (!st) return null;
  const phase = S.savePhase[sid], running = S.running.has(parentId(sid));
  const next = incomingSave(S.saveNext[sid], st, phase, running);
  const b = next ? incomingBadge(next) : saveBadgeInfo(st, phase, running);
  return { ...b, tip: badgeTip(b) };
}

function badgeInner(pageId) {
  const b = saveBadgeFor(pageId);
  if (!b) return '';
  const note = b.note ? `<small>${esc(b.note)}</small>` : '';
  return `<span class="sbadge-line sb-${b.tone}" title="${esc(b.tip)}">${badgeIcon(b.mark)}<b>${esc(b.label)}</b>${note}</span>`;
}

// The status on a game page, above Play. It says changes out loud to a screen reader.
function saveBadge(a) {
  if (!S.library[parentId(a.id)]) return '';
  const sid = saveIdOf(a.id);
  return `<div class="sbadge" role="status" aria-label="${esc(t('Cloud saves'))}" data-save-badge="${esc(sid)}" data-page="${esc(a.id)}">${badgeInner(a.id)}</div>`;
}

// Redraws the status lines on screen (one game's, or all of them), and Big Picture's game page.
function paintSaveBadges(sid) {
  for (const el of $$('[data-save-badge]')) {
    if (sid && el.dataset.saveBadge !== sid) continue;
    el.innerHTML = badgeInner(el.dataset.page);
  }
  if (window.tvRefresh && inTV()) window.tvRefresh();
}

// The Saves tab leads with the same status, so it's drawn again when the status changes. Not from
// paintSaveBadges: drawing the game page paints the badges, and that would go round forever.
function refreshSavesTab(sid) {
  if (inTV() || S.view !== 'game' || S.tab !== 'saves' || !S.saves[S.arg]) return;
  if (saveIdOf(S.arg) === sid) refreshGame();
}

// Signed in or out, or the cloud changed: every game's status is asked again.
function forgetSaveStatus() {
  S.saveStat = {};
  S.savePhase = {};
  S.saveNext = {};
  paintSaveBadges();
}

window.saveBadge = saveBadge;
window.saveBadgeFor = saveBadgeFor;
window.saveBadgeIcon = badgeIcon;
window.paintSaveBadges = paintSaveBadges;

// ---------- the Saves page: every game's saves in one place ----------
const CARD_ICON = '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M7 3h10a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z"/><path d="M9 3v3.5M12 3v3.5M15 3v3.5M8.5 12h7v5h-7z"/></svg>';
let savesAllBusy = null;
async function loadSavesAll() {
  if (savesAllBusy) return savesAllBusy;
  savesAllBusy = (async () => {
    // Never left empty: an empty answer would ask again on every redraw, without end.
    try { S.savesAll = (await invoke('saves_overview')) || { games: [] }; } catch (e) { S.savesAll = { error: String(e && e.message ? e.message : e), games: [] }; }
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
// Where the copies are kept: beside each game in its library (the path shown is the library's
// .backups folder), or a folder the player picked (another drive, a NAS, a folder Drive, OneDrive
// or Dropbox keeps in sync).
function rootCard(o) {
  const mine = !!o.chosen;
  const note = mine && o.reachable === false ? t('Your folder can\'t be reached right now. Copies stay beside each game until it\'s back, then go to it.') : mine ? t('Copies of your saves go here.') : t('Copies of your saves are kept with each game, in its library\'s .backups folder. Pick a folder on another drive, a NAS, or one that Drive, OneDrive or Dropbox keeps in sync, and they go there instead.');
  return `<div class="card"><div class="set-row"><div class="k"><b>${esc(t('Backup Folder'))}</b><span><span class="path">${esc(mine ? o.chosen : (o.root || ''))}</span><br>${esc(note)}</span></div>
    <span class="acts"><button type="button" class="btn sm" data-act="saves-root-open">${esc(t('Open the folder'))}</button><button type="button" class="btn sm" data-act="saves-root-pick">${esc(t('Change'))}</button>${mine ? `<button type="button" class="btn sm" data-act="saves-root-reset">${esc(t('Use the Usual Place'))}</button>` : ''}</span></div></div>`;
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
      <button type="button" class="icon-btn" data-act="saves-now" data-id="${esc(x.id)}" title="${esc(t('Back up now'))}" aria-label="${esc(t('Back up now'))} · ${esc(a.name)}"${S.running.has(x.id) || x.known === false ? ' disabled' : ''}><svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 20V9m0 0-4.5 4.5M12 9l4.5 4.5M5 4h14"/></svg></button></div>`;
  };
  const empty = !games.length
    ? `<div class="empty">${CARD_ICON.replace('width="20" height="20"', 'width="44" height="44"')}<h2>${esc(t('No games yet'))}</h2><p>${esc(t('Install a game and play it once. Its saves are copied here after every session.'))}</p></div>`
    : '';
  return `<div class="page saves-page"><div class="head"><div><h1>${esc(t('Saves'))}</h1><p>${esc(t('After every session the launcher keeps a copy of each game\'s saves. Pick a game to restore an older one.'))}</p></div></div>
    ${o.error ? `<div class="card"><p style="margin:0">${esc(t(o.error))}</p></div>` : ''}
    <div class="mcs">${pcCard}${cloudCard}</div>
    ${rootCard(o)}
    ${withSaves.length ? `<section class="shelf"><div class="shelf-h"><h2>${esc(t('Games'))}</h2></div><div class="slots">${withSaves.map(slot).join('')}</div></section>` : ''}
    ${none.length ? `<section class="shelf"><div class="shelf-h"><h2>${esc(t('No saves yet'))}</h2><span class="muted">${esc(t('Play them once and their saves show up here.'))}</span></div><div class="slots dim">${none.map((x) => { const a = x.a || { id: x.id, name: x.g.name }; return `<button type="button" class="slot-main mini" data-act="saves-game" data-id="${esc(x.id)}"><span class="slot-art">${a.cover ? pic(a.cover) : ''}</span><span class="slot-txt"><b>${esc(a.name)}</b><span>${esc(x.found ? t('Saves found, copied after you play') : t('Not played yet'))}</span></span></button>`; }).join('')}</div></section>` : ''}
    ${empty}</div>`;
}
window.pageSaves = pageSaves;
window.loadSavesAll = loadSavesAll;

// ---------- cloud saves for every game ----------
// Asked once: right after signing in (first-run setup asks on a step of its own instead), and on
// the first start after the update for players who were signed in already. ui.cloud_all: true,
// every game's saves go up; false, only the games switched on in their Manage tab; not set, not
// asked yet (off). A game either follows this or was switched in its Manage tab since it was
// picked: picking again (here or in Settings, Account) makes every game follow it.
const CA_SVG = (paths, size = 22) => `<svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${paths}</svg>`;
const CA = {
  pc: CA_SVG('<rect x="3" y="4" width="18" height="12" rx="2"/><path d="M8 20h8M12 16v4"/>'),
  any: CA_SVG('<rect x="4.5" y="5" width="15" height="10.5" rx="1.5"/><path d="M2.5 19h19"/>'),
  cloud: CA_SVG('<path d="M7 18a4.5 4.5 0 0 1-.6-9 6 6 0 0 1 11.5 1.6A3.8 3.8 0 0 1 17.5 18z"/><path d="M12 15.5v-5m-2.2 2.2L12 10.5l2.2 2.2"/>', 40),
  all: CA_SVG('<path d="M7 18a4.5 4.5 0 0 1-.6-9 6 6 0 0 1 11.5 1.6A3.8 3.8 0 0 1 17.5 18z"/><path d="m9.5 13.5 2 2 3.2-3.6"/>', 26),
  pick: CA_SVG('<path d="M10 6.5h10M10 12h10M10 17.5h10"/><path d="m3.5 6.5 1.5 1.5 2.5-3"/><circle cx="5.2" cy="12" r=".6"/><circle cx="5.2" cy="17.5" r=".6"/>', 26),
  chev: CA_SVG('<path d="m9 6 6 6-6 6"/>', 18),
};
const CLOUD_ALL_LEAD = N_('After you play, a game\'s saves can go to your account, ready on any PC. A copy always stays on this PC.');
let cloudAsked = false;

async function setCloudAll(on) {
  try { await invoke('saves_cloud_follow_all'); } catch (e) { return fail(e); }
  for (const g of Object.values(S.library)) g.cloud = null;
  await saveUi({ cloud_all: on }, true);
  // Each game's Saves tab and badge knew the old answer.
  S.saves = {}; S.savesAll = null; forgetSaveStatus();
}

// Waits until nothing else covers the window (setup, signing in, another question).
function noModal() {
  const free = () => !document.querySelector('.scrim, .oob');
  if (free()) return Promise.resolve();
  return new Promise((ok) => {
    const o = new MutationObserver(() => { if (free()) { o.disconnect(); ok(); } });
    o.observe(document.body, { childList: true });
  });
}

// The two answers, the same words on the desktop and in Big Picture mode.
function cloudAllChoices() {
  return [
    { v: true, icon: CA.all, tvIcon: 'cloud', label: t('Every game'), tag: t('Recommended'), sub: t('Backed up after you play. Turn it off for one game in its Manage tab.') },
    { v: false, icon: CA.pick, tvIcon: 'picklist', label: t('Only games I pick'), sub: t('Saves stay on this PC. Turn cloud saves on for a game in its Manage tab.') },
  ];
}

// true, false, or null when closed without an answer.
async function pickCloudAll() {
  const choices = cloudAllChoices();
  const name = (S.account.user && S.account.user.username) || '';
  const lead = `${tf('Signed in as {name}.', { name })} ${t(CLOUD_ALL_LEAD)}`;
  if (inTV() && window.tvCloudAll) return window.tvCloudAll(t('Back up your saves?'), lead, choices);
  const foot = t('You can change this any time in Settings → Account.');
  const row = (c, i) => `<button type="button" class="ca-pick${c.tag ? ' rec' : ''}" data-x="c${i}"><span class="ic">${c.icon}</span><span class="k"><b>${esc(c.label)}${c.tag ? `<em>${esc(c.tag)}</em>` : ''}</b><span>${esc(c.sub)}</span></span>${CA.chev}</button>`;
  const art = `<div class="ca-art" aria-hidden="true"><span class="dev">${CA.pc}<small>${esc(t('This PC'))}</small></span><i class="ln"></i><span class="cl">${CA.cloud}</span><i class="ln"></i><span class="dev">${CA.any}<small>${esc(t('Any PC'))}</small></span></div>`;
  return new Promise((res) => {
    let picked = null;
    const box = modal(`${art}<div class="ca-body"><h2>${esc(t('Back up your saves?'))}</h2><p>${esc(lead)}</p><div class="ca-picks">${choices.map(row).join('')}</div><p class="ca-foot">${esc(foot)}</p></div>`, (x, close) => {
      const c = choices[Number(String(x.dataset.x).slice(1))];
      if (c) { picked = c.v; close(); }
    }, t('Back up your saves?'));
    $('.modal', box).classList.add('ca');
    new MutationObserver((m, o) => { if (!box.isConnected) { o.disconnect(); res(picked); } }).observe(document.body, { childList: true });
  });
}

async function askCloudAll() {
  const ui = S.settings.ui || {};
  if (cloudAsked || ui.cloud_all != null || !S.settings.setup_done) return;
  if (!(S.account && S.account.signedIn) || document.body.classList.contains('first-run')) return;
  cloudAsked = true;
  await noModal();
  if ((S.settings.ui || {}).cloud_all != null) return;
  const v = await pickCloudAll();
  // Closed without an answer: asked again the next time the launcher opens.
  if (v == null) return;
  await setCloudAll(v);
  toast(v ? t('Every game\'s saves go to your account after you play.') : t('Only the games you turn on back up to your account.'));
  if (['settings', 'game', 'saves'].includes(S.view)) render(true);
}
window.askCloudAll = askCloudAll;
window.setCloudAll = setCloudAll;
window.cloudAllChoices = cloudAllChoices;

// ---------- before Play ----------
// Brings in a newer save from another PC, or asks which to keep. false: don't start it.
// `cards`: on the desktop the choices are big cards to click (a heading, a green tag such as
// Newer, and their `lines`), not buttons under the text. Big Picture's sheet is the same either way.
async function ask(title, text, buttons, cards) {
  if (inTV() && window.tvAsk) {
    // A button marked last (one that changes things for good) goes to the end of the list, away from the first focus.
    const list = buttons.filter((b) => !b.last).concat(buttons.filter((b) => b.last));
    const i = await window.tvAsk(title, list.map((b) => ({ label: b.label, sub: b.sub, tag: b.tag, icon: b.icon })));
    return i == null || i < 0 ? null : list[i].v;
  }
  const card = (b, i) => `<button type="button" class="pick-card pick-btn" data-x="b${i}"><b>${esc(b.label)}${b.tag ? ` <em class="pick-newer">${esc(b.tag)}</em>` : ''}</b>${(b.lines || []).map((l) => `<span>${esc(l)}</span>`).join('')}</button>`;
  const button = (b, i) => `<button type="button" class="btn${b.main ? ' main' : ''}" data-x="b${i}">${esc(b.label)}</button>`;
  const choices = cards ? `<div class="pick-two">${buttons.map(card).join('')}</div>` : `<div class="acts ${buttons.length === 3 ? 'grid3' : 'grid2'}">${buttons.map(button).join('')}</div>`;
  return new Promise((res) => {
    let picked = null;
    const box = modal(`<h2>${esc(title)}</h2>${text}${choices}`, (x, close) => {
      const i = Number(String(x.dataset.x).slice(1));
      if (!isNaN(i) && buttons[i]) { picked = buttons[i].v; close(); }
    }, title);
    // The button marked focus starts focused (not one that changes things for good, which may come first).
    const f = buttons.findIndex((b) => b.focus);
    if (f >= 0) { const el = box.querySelector(`[data-x="b${f}"]`); if (el) el.focus(); }
    new MutationObserver((m, o) => { if (!box.isConnected) { o.disconnect(); res(picked); } }).observe(document.body, { childList: true });
  });
}
async function beforePlay(id) {
  if (!(await beforeCloudPlay(id))) return false;
  const title = saveShareTitle(id);
  if (!title || !saveShareOn(title)) return true;
  let offer;
  try { offer = await invoke('saves_share', { id, selection: null }); } catch (e) { fail(e); return true; }
  if (!offer) return true;
  const current = offer.destinationWhen == null ? t('No save yet') : whenText(offer.destinationWhen);
  const newer = whenText(offer.sourceWhen);
  const compatibility = t('Copy only if both versions support the same save format.');
  const picked = await ask(tf('Use the newer save from {name}?', { name: offer.name }),
    `<div class="pick-two"><div class="pick-card"><b>${esc(t('This version'))}</b><span>${esc(current)}</span></div><div class="pick-card"><b>${esc(offer.name)}</b><span>${esc(newer)}</span></div></div><p>${esc(compatibility)} ${esc(t('If you copy it, the current save is backed up first.'))}</p>`,
    [{ label: t('Keep saves separate'), sub: current, v: 'keep', focus: true },
      { label: t('Copy the newer save'), sub: `${newer} · ${compatibility}`, v: 'copy', main: true },
      { label: t('Turn off sharing'), v: 'off', last: true }]);
  if (picked == null) return false;
  if (picked === 'off' && saveShareOn(title)) await toggleSaveShare(title);
  if (picked === 'copy') {
    try { await invoke('saves_share', { id, selection: offer.selection }); } catch (e) { fail(e); return false; }
    savesChanged(id);
    toast(t('The newer save was copied.'));
  }
  return true;
}
// A newer save from another PC is put in place before the game starts. Nothing is asked here:
// whether a game's saves go up was picked once for every game (askCloudAll), or in its Manage tab.
async function beforeCloudPlay(id) {
  const g = S.library[id];
  if (!g || !(S.account && S.account.signedIn)) return true;
  if (!cloudOn(g)) return true;
  let r;
  try { r = await invoke('saves_check', { id }); } catch (e) { fail(e); return false; }
  if (r.action === 'restored') toast(tf('Your newer save from {device} is in place.', { device: r.cloud.device || t('another PC') }));
  else if (r.action === 'offline') toast(t('Cloud saves can\'t be reached right now. Playing with this PC\'s save.'), true);
  else if (r.action === 'conflict') {
    try { if (!(await pickCloudSave(id, r))) return false; } catch (e) { fail(e); return false; }
  }
  savesChanged(id);
  return true;
}

async function pickCloudSave(id, { local, cloud }) {
  const v = await chooseSave(local, cloud);
  if (v !== 'local' && v !== 'cloud') return false;
  await invoke('saves_pick', { id, keep: v, localHash: local.hash, cloudVersion: cloud.v });
  toast(t(v === 'cloud' ? 'The cloud save is in place.' : 'Your save is in the cloud.'));
  return true;
}

// Which of the two saves is newer ('local' or 'cloud'), or '' when they were saved at the same time.
function newerSave(local, cloud) {
  if (local.when > cloud.made) return 'local';
  if (cloud.made > local.when) return 'cloud';
  return '';
}

// Both saves changed: the player picks one. On the desktop the two saves are the choices
// themselves (cards to click, no buttons under them); in Big Picture they're the sheet's two
// items. The newer one says so, and what happens to the other is said under the question.
function chooseSave(local, cloud) {
  const newer = newerSave(local, cloud);
  const tag = (k) => (k === newer ? t('Newer') : '');
  const played = (secs) => tf('Played {time} in all', { time: fmtTime(secs) });
  const note = `<p class="muted pick-note">${esc(t('The other one is kept as a copy on its Saves tab, so nothing is lost.'))}</p>`;
  return ask(t('Which save do you want?'), note, [
    { label: t('Keep this PC\'s'), sub: `${whenText(local.when)} · ${local.device}`, lines: [whenText(local.when), local.device, played(local.played)], tag: tag('local'), icon: 'desk', v: 'local' },
    { label: t('Use the cloud one'), sub: `${whenText(cloud.made)} · ${cloud.device}`, lines: [whenText(cloud.made), cloud.device, played(cloud.played)], tag: tag('cloud'), icon: 'cloud', v: 'cloud' },
  ], true);
}

async function syncCloudNow(id) {
  const r = await invoke('saves_check', { id, manual: true });
  switch (r.action) {
    case 'conflict': return pickCloudSave(id, r);
    case 'restored': return toast(t('The cloud save is in place.'));
    case 'uploaded': return toast(t('Your save is in the cloud.'));
    case 'offline': return toast(t('Cloud saves can\'t be reached right now.'), true);
    default: return toast(t('Saves are up to date.'));
  }
}

// ---------- actions ----------
window.accountAct = async (act, el) => {
  const id = el.dataset.id;
  switch (act) {
    case 'acc-signin': return signIn(false);
    case 'acc-create': return signIn(true);
    case 'acc-signout': return signOut();
    case 'acc-retry': {
      el.disabled = true;
      // Asked now, even while the launcher is leaving the website alone after trouble; copies
      // waiting go up if it answers.
      let ok = true;
      try { await invoke('cloud_retry', { id: null }); } catch (e) { ok = false; toast(String(e && e.message ? e.message : e), true); }
      await loadAccount(true);
      if (ok && S.account && !S.account.offline) toast(t('Connected again.'));
      return render(true);
    }
    case 'saves-sync': {
      if (S.saves[id]?.busy) return;
      el.disabled = true;
      if (S.saves[id] && S.saves[id] !== 'loading') S.saves[id].busy = true;
      refreshGame();
      try { await syncCloudNow(id); } catch (e) { fail(e); }
      return loadSaves(id);
    }
    case 'acc-cloud-all': {
      const on = (S.settings.ui || {}).cloud_all === true;
      await setCloudAll(!on);
      return render(true);
    }
    case 'acc-device-save': {
      const v = (($('#device-name') || {}).value || '').trim();
      await saveUi({ device_name: v });
      return toast(v ? t('Saved. New saves from this PC carry that name.') : t('Back to the computer\'s own name.'));
    }
    case 'acc-btn': if (S.account && S.account.signedIn) { S.setTab = 'account'; if (reportsUnread()) loadMyReports(); return go('settings'); } return signIn(false);
    case 'myrep-load': return loadMyReports();
    case 'myrep-send': {
      const ta = $(`textarea[data-myrep="${CSS.escape(String(id))}"]`);
      const text = ta ? ta.value.trim() : '';
      if (!text) return toast(t('Write your answer first.'), true);
      el.disabled = true;
      try {
        const r = await invoke('report_reply', { id: Number(id), text });
        const rep2 = ((S.myReports && S.myReports.list) || []).find((x) => String(x.id) === String(id));
        if (rep2 && r && r.replies) rep2.replies = r.replies;
        toast(t('Sent to the developer.'));
      } catch (e) { el.disabled = false; return fail(e); }
      return render(true);
    }
    case 'saves-cloud': {
      const on = el.getAttribute('aria-checked') !== 'true';
      try { S.library[id] = await invoke('saves_cloud', { id, on }); } catch (e) { return fail(e); }
      toast(on ? t('Cloud saves are on for it.') : t('Cloud saves are off for it. What\'s in the cloud stays there.'));
      return loadSaves(id);
    }
    case 'saves-share': {
      const title = saveShareTitle(id);
      if (title) await toggleSaveShare(title);
      return refreshGame();
    }
    case 'saves-now': {
      el.disabled = true;
      try { const c = await invoke('saves_backup', { id }); toast(c ? t('Backed up.') : t('Nothing changed since the last copy.')); } catch (e) { fail(e); }
      if (S.view === 'saves') { el.disabled = false; return loadSavesAll(); }
      return loadSaves(id);
    }
    case 'saves-game': closeMenu(); S.nextTab = 'saves'; return go('game', id, el.querySelector('.slot-art'));
    case 'saves-root-open': return invoke('saves_open_root').catch(fail);
    case 'saves-root-pick': case 'saves-root-reset': {
      let path = null;
      if (act === 'saves-root-pick') {
        const p = await pickFile({ title: t('A folder for the copies of your saves'), directory: true }).catch(fail);
        if (!p) return;
        path = typeof p === 'string' ? p : p.path;
      }
      el.disabled = true;
      let r;
      try { r = await invoke('saves_set_root', { path }); } catch (e) { el.disabled = false; return fail(e); }
      S.settings.save_dir = r.custom ? r.path : null;
      toast(!r.custom ? t('Copies of your saves are kept with each game again.') : r.moved ? tn(r.moved, 'One game\'s copies moved there. New ones go there too.', '{n} games\' copies moved there. New ones go there too.') : t('New copies of your saves go there from now on.'));
      return loadSavesAll();
    }
    case 'saves-more': S.savesMore.add(id); return refreshGame();
    case 'saves-less': S.savesMore.delete(id); return refreshGame();
    case 'saves-cloud-delete': {
      const v = await ask(t('Delete this save from the cloud?'), `<p>${esc(t('It\'s gone from your account for good. Copies on this PC stay.'))}</p>`,
        [{ label: t('Cancel'), v: null, focus: true }, { label: t('Delete'), v: 'yes', main: true }]);
      if (v !== 'yes') return;
      try { await invoke('saves_cloud_delete', { id, v: el.dataset.v }); toast(t('Deleted from the cloud.')); } catch (e) { fail(e); }
      return loadSaves(id);
    }
    case 'saves-open': return invoke('saves_open', { id }).catch(fail);
    case 'saves-open-backups': return invoke('saves_open_backups', { id }).catch(fail);
    case 'saves-place': {
      // The player knows better (a game that saves somewhere else): their folder from now on.
      const p = await pickFile({ title: t('The folder where the game keeps its saves'), directory: true }).catch(fail);
      if (!p) return;
      try { S.library[id] = await invoke('saves_set_place', { id, path: typeof p === 'string' ? p : p.path }); } catch (e) { return fail(e); }
      toast(t('Saved. Backups and cloud saves use that folder now.'));
      return loadSaves(id);
    }
    case 'saves-place-add': {
      const p = await pickFile({ title: t('Another folder where the game keeps its saves'), directory: true }).catch(fail);
      if (!p) return;
      try { S.library[id] = await invoke('saves_add_place', { id, path: typeof p === 'string' ? p : p.path }); } catch (e) { return fail(e); }
      toast(t('Added. Backups and cloud saves cover that folder too.'));
      return loadSaves(id);
    }
    case 'saves-place-remove': try { S.library[id] = await invoke('saves_remove_place', { id, template: el.dataset.t || '' }); } catch (e) { return fail(e); } return loadSaves(id);
    case 'saves-restore': {
      const v = await ask(t('Restore this save?'), `<p>${esc(t('The game goes back to this save. What\'s there now is kept as a copy first, so you can switch back.'))}</p>`,
        [{ label: t('Cancel'), v: null }, { label: t('Restore'), v: 'yes', main: true }]);
      if (v !== 'yes') return;
      try { await invoke('saves_restore', { id, name: el.dataset.name || null, v: el.dataset.v || null }); toast(t('That save is back in place.')); } catch (e) { fail(e); }
      return loadSaves(id);
    }
  }
  return null;
};

T.event.listen('account', ({ payload }) => { if (payload && payload.signedIn === false) { S.account = { signedIn: false }; S.saves = {}; forgetSaveStatus(); renderAccountBits(); if (window.afterSignIn) window.afterSignIn(); } else loadAccount(true); });
T.event.listen('saves', ({ payload }) => { if (S.view === 'saves') loadSavesAll(); if (payload && payload.id) { delete S.saves[payload.id]; loadSaveStatus(payload.id); if (S.view === 'game' && S.arg === payload.id && S.tab === 'saves') loadSaves(payload.id); } });
// A check, upload or download for a game started or ended: the badge says so, and asks for the
// outcome once it's over.
T.event.listen('save-sync', ({ payload }) => {
  const sid = payload && payload.id;
  if (!sid) return;
  if (payload.phase !== 'idle') {
    S.savePhase[sid] = payload.phase;
    paintSaveBadges(sid);
    return refreshSavesTab(sid);
  }
  delete S.savePhase[sid];
  loadSaveStatus(sid);
});

// Who's signed in: what's remembered first, then the website's answer.
loadAccount(false).then(() => loadAccount(true));
