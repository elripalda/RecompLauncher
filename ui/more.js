'use strict';
// RecompLauncher's own updates (found while it runs, installed quietly, What's new after), games
// sent from recomplauncher.com ("Get it on my PC"), betas of recomps, "How did it run?" after a
// session, and usage and diagnostics. Uses app.js's helpers and state (S), and account.js's ask().

S.betas = [];
const tvOn = () => document.body.classList.contains('tv-on');

// ---------- updates ----------
T.event.listen('launcher-update', ({ payload: v }) => { if (v && v.version) { S.launcherUpd = v; renderSide(); if (S.view === 'downloads') render(true); } if (v && v.version && v.version !== LL.told) { LL.told = v.version; updateToast(v); } });
// Pushed by the website: a recomp's new release (its releases were just looked at again). A changed
// list comes as 'catalog-fresh' (app.js).
T.event.listen('push', ({ payload: p }) => {
  if (p && p.type === 'game') checkUpdates();
});
// The download, then the install (no window of its own); RecompLauncher opens again after.
let updBox = null;
T.event.listen('self-update', ({ payload: p }) => {
  // The update window (app.js) shows the download in its own button.
  const win = $('.upd-start.busy');
  if (win) return p.failed ? updWinReset(win) : updWinProgress(win, p);
  if (p.failed) { if (updBox) updBox.remove(); updBox = null; return; }
  if (!updBox || !updBox.isConnected) {
    updBox = modal(`<div class="upd"><h2>${esc(t('Updating RecompLauncher'))}</h2><div class="prog"><div class="track"><i style="width:0%"></i></div></div><p class="muted" data-st>${esc(t('Downloading…'))}</p></div>`, () => {}, t('Updating RecompLauncher'));
    if (S.os === 'linux') updBox.dataset.busy = 'true';
  }
  const pct = p.total ? Math.round((p.done / p.total) * 100) : 0;
  const bar = $('.track i', updBox), st = $('[data-st]', updBox);
  if (bar) bar.style.width = (p.installing ? 100 : pct) + '%';
  if (st) st.textContent = p.installing ? t('Installing. RecompLauncher opens again in a moment.') : p.total ? tf('{done} of {total}', { done: fmtB(p.done), total: fmtB(p.total) }) : t('Downloading…');
});
// The Featured Banner on Home is at least 340px tall, up to 520px (40% of the width, app.css
// .slider). On a small screen (a 1366×768 laptop, 1080p at 125% scaling) that is more than half of
// it, so a new install there starts with the banner off; Settings → Appearance turns it back on.
// Asked only for a new install (below), so a player who runs setup again keeps the banner they had.
function bannerTooBig() {
  const banner = Math.min(520, Math.max(340, screen.width * 0.4));
  return banner > screen.height / 2;
}
// What's new: once, the first time a new version opens.
async function whatsNew() {
  const ui = S.settings.ui || {};
  if (ui.seen_version === S.version) return;
  // A new install has nothing to compare with: just remember this version.
  const newInstall = !ui.seen_version && !S.settings.setup_done;
  if (newInstall) {
    const first = { seen_version: S.version };
    if (bannerTooBig()) first.hide_hero = true;
    return saveUi(first);
  }
  const from = ui.seen_version;
  // Show What's New After an Update off (Settings → Updates and Downloads): the notes stay in
  // Settings → Changelog.
  if (ui.whats_new_off) return saveUi({ seen_version: S.version });
  let v = null;
  try { v = await invoke('launcher_notes'); } catch (e) { /* offline: next time */ return; }
  await saveUi({ seen_version: S.version });
  if (!v || v.version !== S.version || !v.notes) return;
  if (tvOn()) {
    modal(`<div class="whatsnew"><h2>${esc(tf('What\'s new in RecompLauncher {version}', { version: verName(S.version) }))}</h2><div class="md">${md(v.notes, true)}</div>
      <div class="acts"><button type="button" class="btn main" data-x="no">${esc(t('Got it'))}</button></div></div>`, () => {}, t('What\'s new'));
    return;
  }
  whatsNewCard(v, from);
}
// After an update, on the desktop: a small card in the corner instead of a window, since the
// update window already showed the notes before. Its points are the first three Added or Improved
// lines of the notes (their bold part), the rest is a count. It goes after ten seconds, but not
// while the pointer is on it, and leaves nothing behind.
const WN_MS = 10000;
function whatsNewCard(v, from) {
  const items = noteItems(v.notes);
  const picks = items.filter((x) => x.verb !== 'Fixed' && x.title).slice(0, 3);
  const more = items.length - picks.length;
  const points = picks.map((x) => `<li><span>${esc(x.title)}</span></li>`).join('') + (picks.length && more ? `<li class="more"><span>${esc(tn(more, 'and 1 more change', 'and {n} more changes'))}</span></li>` : '');
  const card = document.createElement('div');
  card.className = 'wn-card';
  card.setAttribute('role', 'status');
  card.innerHTML = `<div class="wn-top"><img class="wn-mark" src="img/mark.svg" alt=""><div class="wn-h"><h3>${esc(tf('Updated to {version}', { version: verName(S.version) }))}</h3>${from ? `<span>${esc(tf('From {version}', { version: verName(from) }))}</span>` : ''}</div>
      <button type="button" class="wn-x" data-x="close" aria-label="${esc(t('Close'))}"><svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18"/></svg></button></div>
    ${points ? `<ul>${points}</ul>` : ''}
    <div class="wn-bot"><label class="wn-chk"><input type="checkbox" data-x="off">${esc(t('Don\'t show this after updates'))}</label><button type="button" class="btn sm main" data-x="open">${esc(t('See What\'s New'))}</button></div>`;
  document.body.append(card);
  let timer = 0;
  const close = () => { clearTimeout(timer); card.classList.add('out'); setTimeout(() => card.remove(), 300); };
  const arm = () => { clearTimeout(timer); timer = setTimeout(close, WN_MS); };
  card.addEventListener('mouseenter', () => clearTimeout(timer));
  card.addEventListener('mouseleave', arm);
  card.addEventListener('focusin', () => clearTimeout(timer));
  card.addEventListener('focusout', (e) => { if (!card.contains(e.relatedTarget) && !card.matches(':hover')) arm(); });
  card.addEventListener('change', (e) => { if (e.target.dataset.x === 'off') saveUi({ whats_new_off: e.target.checked }, true); });
  card.addEventListener('click', (e) => {
    const b = e.target.closest('button[data-x]');
    if (!b) return;
    close();
    if (b.dataset.x === 'open') setSection('changelog');
  });
  arm();
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
      // Asked where it goes, like an install started here.
      startInstall(id);
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
      return startInstall(id, S.library[id] ? null : undefined, tag);
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
// Reviews need an account (names never show with them). Signed out, a Sign In button instead of a
// failure. The launcher's own words, not the site's: the site still calls them reports.
const SIGNIN_TEXT = N_('Sign in to your RecompLauncher account to write a review. Your name isn\'t shown with it.');
const signedIn = () => !!(S.account && S.account.signedIn);
async function signInToReport() {
  const msg = t(SIGNIN_TEXT);
  if (tvOn() && window.tvAsk) {
    const i = await window.tvAsk(msg, [{ label: t('Sign In'), icon: 'check' }, { label: t('Not now'), icon: 'chevL' }]);
    if (i === 0 && window.signIn) window.signIn(false);
    return;
  }
  modal(`<h2>${esc(t('Sign In to Write Reviews'))}</h2><p>${esc(msg)}</p><div class="acts grid2"><button type="button" class="btn" data-x="no">${esc(t('Not Now'))}</button><button type="button" class="btn main" data-x="in">${esc(t('Sign In'))}</button></div>`, (x, close) => {
    if (x.dataset.x === 'in') { close(); if (window.signIn) window.signIn(false); }
  }, t('Sign In to Write Reviews'));
}
const needSignIn = (e) => { const m = String(e && e.message ? e.message : e); return m.startsWith('NEED_SIGNIN|') ? m.slice(12) : null; };
window.signInToReport = signInToReport;
const VERDICT = [['runs', N_('Plays great')], ['issues', N_('Has problems')], ['broken', N_('Won\'t run')]];
// What each answer means, under it on its card, and its picture.
const VERDICT_SUB = { runs: N_('Ran smoothly, start to finish'), issues: N_('Glitches, crashes or slowdowns'), broken: N_('Doesn\'t start, or can\'t be played') };
const VERDICT_ICON = {
  runs: '<circle cx="12" cy="12" r="9"/><path d="m8 12.4 2.8 2.8L16.2 9.8"/>',
  issues: '<path d="M10.3 4.3 2.9 17.4A2 2 0 0 0 4.6 20.4h14.8a2 2 0 0 0 1.7-3L13.7 4.3a2 2 0 0 0-3.4 0z"/><path d="M12 9.6v4.2M12 17.2h.01"/>',
  broken: '<circle cx="12" cy="12" r="9"/><path d="m9.2 9.2 5.6 5.6m0-5.6-5.6 5.6"/>',
};
const rvIcon = (k) => `<svg width="26" height="26" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${VERDICT_ICON[k]}</svg>`;
// Asking after a long session is the player's to turn off (Don't Ask Me Again, or Settings >
// Reviews, which turns it back on). On until they do.
const reviewAskOn = () => (S.settings.ui || {}).review_ask !== false;
const setReviewAsk = (on) => saveUi({ review_ask: !!on }, true);
// A game the account has a review of already, written on this PC or any other.
const reviewedAlready = (id) => ((S.myReports && S.myReports.list) || []).some((x) => x.app === id);
Object.assign(window, { reviewAskOn, setReviewAsk });
async function askRun(id, secs, asked_for) {
  const g = S.library[id];
  if (!g || g.local) return;
  // Asked after a long session until the game has a review: after the first long session of each
  // version, and if that goes unanswered, after three more, three times at most. Once reviewed it's
  // never asked about again, and nothing is asked once the player turned asking off.
  const asked = (S.settings.ui || {}).ran || {};
  if (!asked_for && (!reviewAskOn() || asked[id] === 'never' || reviewedAlready(id))) return;
  // Signed out it can't be sent: not asked after a session, and the review button asks to sign in.
  if (!signedIn()) return asked_for ? signInToReport() : undefined;
  const was = asked[id];
  const sentBefore = !!(was && typeof was === 'object' && was.sent);
  let r = was && typeof was === 'object' ? Object.assign({}, was) : was ? { tag: was, asks: 1, waits: 0, sent: false } : null;
  if (!r || r.tag !== g.tag) r = { tag: g.tag, asks: 0, waits: 0, sent: sentBefore };
  const keep = (x) => saveUi({ ran: Object.assign({}, (S.settings.ui || {}).ran || {}, { [id]: x }) }, true);
  if (!asked_for) {
    if (r.sent || r.asks >= 3) return;
    if (r.asks > 0 && r.waits < 3) { r.waits++; return keep(r); }
    r.asks++; r.waits = 0;
    await keep(r);
  }
  const mins = Math.round(secs / 60);
  // What goes with it, shown before sending: the PC in a line, the version, and the game's log.
  let pre = null;
  try { pre = await invoke('report_preview', { id }); } catch (e) { pre = null; }
  const send = async (verdict, note, log) => {
    const r2 = await invoke('run_report', { id, verdict, note: note || null, mins, log: !!log });
    // A new review after deleting one: it isn't hidden any more.
    if (S.reviewGone) delete S.reviewGone[id];
    keep(Object.assign({}, r, { sent: true }));
    toast(r2 && r2.warning ? t(r2.warning) : t('Thanks! It\'s on the game\'s page on recomplauncher.com.'));
    delete S.reports[id];
    if (S.view === 'game' && S.arg === id) setTimeout(() => loadReports(id, true), 1500);
    if (S.account && S.account.signedIn && window.loadMyReports) setTimeout(() => window.loadMyReports(true), 2500);
  };
  const specsLine = pre ? [pre.device, pre.specs, pre.version].filter(Boolean).join(' · ') : '';
  if (tvOn() && window.tvAsk) {
    const items = VERDICT.map(([k, n]) => ({ label: t(n), sub: t(VERDICT_SUB[k]), icon: 'check' }));
    // After a session, Don't Ask Me Again too, as on the desktop.
    if (!asked_for) items.push({ label: t('Don\'t Ask Me Again'), sub: t('Turn it back on in Settings, Reviews.'), icon: 'bell' });
    const i = await window.tvAsk(tf('How did {name} run?', { name: reviewName(id, g) }), items.concat([{ label: t('Not now'), icon: 'chevL' }]));
    if (!asked_for && i === 3) return setReviewAsk(false);
    if (i == null || i < 0 || i > 2) return;
    const verdict = VERDICT[i][0];
    const sent = (p) => p.catch((e) => { const s = needSignIn(e); return s != null ? signInToReport() : fail(e); });
    if (verdict === 'runs') return sent(send(verdict));
    // A problem: what happened, typed with the controller, and the game's log along with it.
    const more = `<p class="tv-kb-note">${esc(t('Your name isn\'t shown with your review.'))}</p>${specsLine ? `<p class="tv-kb-note">${esc(t('This goes with your review'))}: ${esc(specsLine)}</p>` : ''}${pre && pre.log ? `<p class="tv-kb-note">${esc(t('The game\'s log goes too. Only the game\'s developer sees it.'))}</p>` : ''}`;
    const note = window.tvType ? await window.tvType({ title: t('What happened?'), hint: t('What happened, and when?'), min: 15, more }) : null;
    if (note == null) return;
    return sent(send(verdict, note, !!(pre && pre.log)));
  }
  // After a session it waits for any other window the launcher has open (a save to pick, say).
  if (!asked_for && typeof noModal === 'function') await noModal();
  // Changing a review: the dialog starts from what was said before.
  const prev = asked_for ? myReviewOf(id) : null;
  reviewDialog({ id, g, secs: asked_for ? 0 : secs, auto: !asked_for, pre, specsLine, send, prev });
}
// The review, in front of everything: the game's art and the question on top, the three answers as
// cards, then (in the same window) a few words and what goes with them. Asked after a session, it
// also has Don't Ask Me Again, which turns asking off for every game until Settings > Reviews turns
// it back on. While the player is writing, a click outside or Escape doesn't throw the words away.
// Takes the player's review back: off the project's page and out of Your Reviews. The launcher still
// doesn't ask about the game after a session (it was reviewed once; ran[id] keeps sent).
async function deleteReview(id) {
  const mine = myReviewOf(id);
  await invoke('review_delete', { id });
  // Off the screen at once, and kept off while the website's lists catch up (goneReview).
  goneReview(id, mine);
  if (S.myReports && S.myReports.list) S.myReports.list = S.myReports.list.filter((x) => x.app !== id);
  toast(t('Your review is deleted.'));
  if (S.view === 'game' && S.arg === id) { refreshGame(); loadReports(id, true); }
  if (S.view === 'home' || S.view === 'discover') render(true);
  if (S.view === 'settings') render(true);
  if (window.loadMyReports) setTimeout(() => window.loadMyReports(true), 1500);
}
// A review is about the project (the recomp), so the question names it, not the game: the catalog's
// own name, which Show Game Names keeps aside while the game's shows on the page.
function reviewName(id, g) {
  const a = byId(id);
  if (!a) return g.name;
  return (a._name !== undefined ? a._name : a.name) || g.name;
}
function reviewDialog({ id, g, secs, auto, pre, specsLine, send, prev }) {
  const a = byId(id) || {};
  // The project's own pictures, not the game's that Show Game Media or Show Game Names put on its page.
  const own = (k) => pickArt(a, k, false);
  const hero = media(thumbOf(own('hero') || own('cover'), 960)), cover = media(thumbOf(own('cover'), 160));
  const howTo = /^https:\/\//i.test(String(a.report_url || '')) ? a.report_url : '';
  // Opened from the game page (not after a session): it says whether it's a new review or a change.
  const editing = !auto && !!prev;
  const kind = auto ? '' : editing ? t('Edit Your Review') : t('Write a Review');
  const session = secs >= 60 ? `<small class="rv-when">${esc(tf('{time} this session', { time: fmtTime(secs) }))}</small>`
    : kind ? `<small class="rv-when rv-kind">${esc(kind)}</small>` : '';
  // The version it's about: a review sent again is about the version installed now.
  const nowVer = (pre && pre.version) || g.tag || '';
  const verNote = editing && prev.version && nowVer && prev.version !== nowVer
    ? `<p class="rv-vernote">${esc(tf('Your review is about {old}. Sending it again makes it about {new}.', { old: prev.version, new: nowVer }))}</p>` : '';
  // What goes with it, a chip each, so it reads at a glance.
  const specs = specsLine ? `<div class="rv-specs"><small>${esc(t('This goes with your review'))}</small><div>${specsLine.split(' · ').filter(Boolean).map((x) => `<span>${esc(x)}</span>`).join('')}</div></div>` : '';
  const sendLabel = editing ? t('Update Review') : t('Send');
  let draft = prev && prev.note ? prev.note : '', logTouched = false;
  // The X in the corner does what Cancel (or Not Now) does.
  const shut = `<button type="button" class="rv-x" data-x="cancel" aria-label="${esc(t('Close'))}" title="${esc(t('Close'))}"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18"/></svg></button>`;
  const art = `<div class="rv-art${hero ? '' : ' plain'}"${hero ? ` style="--rv-hero:url('${esc(hero)}')"` : ''}>${shut}${cover ? `<img class="rv-cover" src="${esc(cover)}" alt="">` : ''}<div class="rv-tt">${session}<h2>${esc(tf('How did {name} run?', { name: reviewName(id, g) }))}</h2></div></div>`;
  const left = auto ? `<label class="rv-never"><button type="button" class="switch" role="switch" aria-checked="${!reviewAskOn()}" data-x="never"></button><span>${esc(t('Don\'t Ask Me Again'))}<small>${esc(t('Turn it back on in Settings, Reviews.'))}</small></span></label>`
    // Changing a review already sent: it can be taken back too (never offered right after a session).
    : prev ? `<button type="button" class="btn danger rv-del" data-x="del">${esc(t('Delete Review'))}</button>` : '<span></span>';
  // A few words: needed for a problem (what happened, and when), up to them for Plays great.
  const isBad = (v) => !!v && v !== 'runs';
  const question = (v) => (isBad(v) ? t('What happened?') : t('Anything others should know?'));
  const hint = (v) => (isBad(v) ? t('What happened, and when? (For example: crashes after the intro on the first level.)') : t('What works, what doesn\'t, settings that helped'));
  const field = (v) => `<textarea rows="4" maxlength="1000" placeholder="${esc(hint(v))}"${isBad(v) ? ' required aria-required="true"' : ''}>${esc(draft)}</textarea>
      <small class="rv-count" aria-live="polite"></small>`;
  const extras = (v) => `${pre && pre.log ? `<label class="ask-log"><input type="checkbox" data-log${isBad(v) ? ' checked' : ''}><span>${esc(t('Attach the Game\'s Log'))}<small>${esc(t('Only the game\'s developer sees the log.'))}</small></span></label>` : ''}
      ${specs}${verNote}
      ${howTo ? `<button type="button" class="linkbtn ask-how" data-x="how">${esc(t('How to Report Bugs for This Game'))}</button>` : ''}
      <p class="ask-err" hidden></p>`;
  const anon = `<small class="ask-anon">${esc(t('Your name isn\'t shown with your review.'))}</small>`;
  // One page: the answers on top, the words under them, Send at the bottom. Edit opens with what was
  // said before, so changing one word is one click away.
  const onePick = ([k, n]) => `<button type="button" class="rv-pick v-${k}${verdict === k ? ' on' : ''}" role="radio" aria-checked="${verdict === k}" data-x="v" data-v="${k}"><span class="rv-ic">${rvIcon(k)}</span><span class="rv-pt"><b>${esc(t(n))}</b><span>${esc(t(VERDICT_SUB[k]))}</span></span></button>`;
  const one = () => `<div class="rv-body rv-one"><div class="rv-picks mini" role="radiogroup" aria-label="${esc(tf('How did {name} run?', { name: reviewName(id, g) }))}">${VERDICT.map(onePick).join('')}</div>
      <div class="rv-write${verdict ? '' : ' off'}"><b class="rv-q">${esc(question(verdict))}</b><div class="rv-field">${field(verdict)}</div>${extras(verdict)}</div>
      <div class="rv-foot">${left}<div class="rv-foot-r"><button type="button" class="btn" data-x="cancel">${esc(auto ? t('Not Now') : t('Cancel'))}</button><button type="button" class="btn main" data-x="send" disabled>${esc(sendLabel)}</button></div></div>
      ${anon}</div>`;
  // Deleting asks once more, in the same window, saying what goes.
  const confirmDel = () => `<div class="rv-body rv-del-q"><b class="rv-q">${esc(t('Delete your review?'))}</b>
      <p class="rv-lead">${esc(tf('Your review of {name} comes off its page and out of Your Reviews, with any answers on it. This can\'t be undone.', { name: reviewName(id, g) }))}</p>
      <p class="ask-err" hidden></p>
      <div class="acts grid2"><button type="button" class="btn" data-x="back">${esc(t('Cancel'))}</button><button type="button" class="btn main bad" data-x="del-yes">${esc(t('Delete Review'))}</button></div></div>`;
  let verdict = prev ? prev.verdict : null, busy = false;
  // Send is ready once there's an answer and, for a problem, enough words; the count under the field
  // says how many more it needs. A note from before counts.
  const wire = () => {
    const box = $('.modal', s), ta = $('textarea', box);
    if (!ta) return;
    const upd = () => {
      const need = isBad(verdict) ? Math.max(0, 15 - [...ta.value.trim()].length) : 0;
      const go2 = $('[data-x="send"]', box);
      if (go2) go2.disabled = !verdict || need > 0;
      const c = $('.rv-count', box);
      if (c) c.textContent = !verdict ? t('Pick an answer first') : need ? tf('{n} more letters', { n: need }) : '';
    };
    ta.oninput = () => { draft = ta.value; s.dataset.busy = '1'; upd(); };
    const log = $('[data-log]', box);
    if (log) log.onchange = () => { logTouched = true; };
    upd();
  };
  const s = modal(`${art}${one()}`, async (x, close) => {
    const box = $('.modal', s);
    const body = () => $('.rv-body', box);
    const act = x.dataset.x;
    if (act === 'never') {
      const off = x.getAttribute('aria-checked') !== 'true';
      x.setAttribute('aria-checked', String(off));
      return setReviewAsk(!off);
    }
    // Cancel is said on purpose: it closes even with words written (a click outside doesn't).
    if (act === 'cancel') { if (busy) return; delete s.dataset.busy; return close(); }
    if (act === 'v') {
      // Picked in place: the words stay, the question and what's needed follow the answer.
      verdict = x.dataset.v;
      for (const p of $$('.rv-pick', box)) { const on = p.dataset.v === verdict; p.classList.toggle('on', on); p.setAttribute('aria-checked', String(on)); }
      $('.rv-write', box).classList.remove('off');
      $('.rv-q', box).textContent = question(verdict);
      const ta = $('textarea', box);
      ta.placeholder = hint(verdict);
      ta.required = isBad(verdict);
      const log = $('[data-log]', box);
      if (log && !logTouched) log.checked = isBad(verdict);
      wire();
      ta.setSelectionRange(ta.value.length, ta.value.length);
      return ta.focus();
    }
    // Back from Delete's question: the review as it was being written.
    if (act === 'back') {
      if (busy) return;
      body().outerHTML = one();
      return wire();
    }
    if (act === 'del') { const ta = $('textarea', box); if (ta) draft = ta.value; body().outerHTML = confirmDel(); return; }
    if (act === 'del-yes') {
      if (busy) return;
      busy = true;
      $$('.acts .btn', box).forEach((b) => { b.disabled = true; });
      try {
        await deleteReview(id);
        close();
      } catch (ex) {
        if (needSignIn(ex) != null) { close(); return signInToReport(); }
        busy = false;
        const err = $('.ask-err', box);
        err.textContent = t(String(ex && ex.message ? ex.message : ex));
        err.hidden = false;
        $$('.acts .btn', box).forEach((b) => { b.disabled = false; });
      }
      return;
    }
    if (act === 'how') return invoke('open_url', { url: howTo }).catch(fail);
    if (act !== 'send') return;
    const ta = $('textarea', box), err = $('.ask-err', box);
    const note = ta.value.trim();
    if (!verdict || (verdict !== 'runs' && [...note].length < 15)) return;
    if (busy) return;
    busy = true;
    $$('.acts .btn, .rv-foot .btn, .rv-x', box).forEach((b) => { b.disabled = true; });
    try {
      await send(verdict, note, !!($('[data-log]', box) || {}).checked);
      delete s.dataset.busy;
      close();
    } catch (ex) {
      const sn = needSignIn(ex);
      if (sn != null) { delete s.dataset.busy; close(); return signInToReport(); }
      // The site's own words when it turns the review down.
      busy = false;
      err.textContent = String(ex && ex.message ? ex.message : ex);
      err.hidden = false;
      $$('.acts .btn, .rv-foot .btn, .rv-x', box).forEach((b) => { b.disabled = false; });
      wire();
    }
  }, tf('How did {name} run?', { name: reviewName(id, g) }));
  // Focus on the window, not on the first answer, so nothing looks picked before the player picks.
  const box = $('.modal', s);
  box.classList.add('rv');
  box.tabIndex = -1;
  wire();
  box.focus();
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
// Betas: when the launcher opens and when the account changes.
setTimeout(loadBetas, 4000);
T.event.listen('account', () => setTimeout(loadBetas, 1500));
window.askRun = askRun;
