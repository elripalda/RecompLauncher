'use strict';
// Settings > Beta: tell us how the beta is going, and the Discord channels for it.
// Feedback goes to the admin's Inbox (with the other logs) through the beta_feedback command.
(() => {
  const KINDS = [['bug', N_('Something broke')], ['idea', N_('An idea')], ['thoughts', N_('Thoughts')]];
  const MOODS = [['great', N_('Great')], ['good', N_('Good')], ['rough', N_('Rough')], ['broken', N_('Broken')]];
  const AREAS = [['Home and Discover', N_('Home and Discover')], ['Installing and updates', N_('Installing and updates')], ['Playing a game', N_('Playing a game')],
    ['Big Picture mode', N_('Big Picture mode')], ['Saves and cloud saves', N_('Saves and cloud saves')], ['Account and signing in', N_('Account and signing in')],
    ['Settings', N_('Settings')], ['Something else', N_('Something else')]];
  const fb = () => (S.betaFb = S.betaFb || { kind: 'bug', area: 'Something else', mood: '', text: '', contact: '', attach: true, sent: false, busy: false, game: '' });
  // The installed games, by name, for "Which game?".
  const games = () => Object.values(S.library || {}).map((g) => ({ id: g.id, name: (typeof libApp === 'function' ? libApp(g).name : g.name) || g.id })).sort((a, b) => a.name.localeCompare(b.name));
  // A channel's link from the catalog's site settings, or the main invite until there is one.
  const link = (k) => { const u = (S.cat && S.cat.site && S.cat.site[k]) || ''; return /^https:\/\//.test(u) ? u : discordUrl(); };
  const CHANNELS = [
    ['discord_beta', N_('Beta chat'), N_('Talk about the beta with the other testers and see what\'s coming next.')],
    ['discord_bugs', N_('Bug reports'), N_('Post a bug with a screenshot or a clip. Others can say if it happens to them too.')],
    ['discord_ideas', N_('Ideas'), N_('Suggest something for the next beta, or vote on what others asked for.')],
  ];

  function form() {
    const f = fb();
    if (f.sent) {
      return `<div class="bt-sent"><b>${esc(t('Thanks, it\'s in.'))}</b><p>${esc(t('It went straight to the developer. Found something else? Send as many as you like.'))}</p><button type="button" class="btn" data-b="again">${esc(t('Send another'))}</button></div>`;
    }
    const seg = (name, list, cur, label) => `<div class="seg bt-seg" role="radiogroup" aria-label="${esc(label)}">${list.map(([k, n]) => `<button type="button" role="radio" aria-checked="${cur === k}" data-b="${name}" data-k="${k}">${esc(t(n))}</button>`).join('')}</div>`;
    return `<div class="bt-form">
      <div class="bt-row"><span class="bt-l">${esc(t('What is it?'))}</span>${seg('kind', KINDS, f.kind, t('What is it?'))}</div>
      <div class="bt-row"><span class="bt-l">${esc(t('Which game?'))}</span><select class="sel" data-b="game" aria-label="${esc(t('Which game?'))}"><option value="">${esc(t('None, the launcher itself'))}</option>${games().map((x) => `<option value="${esc(x.id)}"${f.game === x.id ? ' selected' : ''}>${esc(x.name)}</option>`).join('')}</select></div>
      <div class="bt-row"><span class="bt-l">${esc(t('Where in the launcher?'))}</span><select class="sel" data-b="area" aria-label="${esc(t('Where in the launcher?'))}">${AREAS.map(([k, n]) => `<option value="${esc(k)}"${f.area === k ? ' selected' : ''}>${esc(t(n))}</option>`).join('')}</select></div>
      <div class="bt-row"><span class="bt-l">${esc(t('How\'s the beta going?'))} <small>${esc(t('optional'))}</small></span>${seg('mood', MOODS, f.mood, t('How\'s the beta going?'))}</div>
      <textarea class="field bt-text" data-b="text" rows="6" maxlength="1800" placeholder="${esc(f.kind === 'bug' ? t('What you did, what you expected, and what happened instead.') : f.kind === 'idea' ? t('What would you add or change, and why?') : t('What you like, what feels off, anything at all.'))}" aria-label="${esc(t('Your feedback'))}">${esc(f.text)}</textarea>
      <div class="bt-two">
        <input class="field" data-b="contact" maxlength="120" value="${esc(f.contact)}" placeholder="${esc(t('Discord name or email, for a reply'))}" aria-label="${esc(t('How can we reach you?'))}">
        <label class="bt-attach"><button type="button" class="switch" role="switch" aria-checked="${!!f.attach}" data-b="attach" aria-label="${esc(t('Include my PC and launcher info'))}"></button><span>${esc(t('Include my PC and launcher info'))}<small>${esc(t('Your PC\'s parts, your installed games and recent downloads. No files, no names.'))}</small></span></label>
      </div>
      <div class="bt-go"><span class="status" role="status"></span><button type="button" class="btn main" data-b="send"${f.busy ? ' disabled' : ''}>${esc(f.busy ? t('Sending…') : t('Send it'))}</button></div>
    </div>`;
  }

  // Settings > Report a Problem: the form, for anything that went wrong (a game picked from a
  // game's right-click menu comes already chosen, with its last log going along).
  window.reportSection = (group) => {
    const f = fb();
    if (S.reportGame) { Object.assign(f, { game: S.reportGame, kind: 'bug', area: 'Playing a game', sent: false }); S.reportGame = ''; }
    return group(t('Report a Problem'), `<p class="muted" style="margin:0 0 12px">${esc(t('It goes straight to the developer. Pick the game if it\'s about one: its last log and how the launcher started it go along.'))}</p><div data-fb>${form()}</div>`);
  };

  window.betaSection = (group, row) => {
    const hero = `<div class="beta-hero"><div class="bh-t"><span class="bh-tag">${esc(t('Beta'))}</span><h2>${esc(tf('RecompLauncher {version}', { version: verName(S.version) }))}</h2>
      <p>${esc(t('Thanks for testing. Tell us what breaks, what feels off and what you\'d change. Every report is read, and it decides what gets fixed first.'))}</p></div>
      <div class="bh-acts"><button type="button" class="btn" data-b="notes">${esc(t('What\'s new in this beta'))}</button><button type="button" class="btn" data-url="${esc(link('discord_beta'))}">${DISCORD}${esc(t('Beta on Discord'))}</button></div></div>`;
    return hero
      + group(t('Send feedback'), row(t('Report a Problem'), t('Something broke, an idea, or what you think so far. It goes straight to the developer.'), `<button type="button" class="btn main" data-b="toreport">${esc(t('Report a Problem'))}</button>`))
      + group(t('On Discord'), CHANNELS.map(([k, n, d]) => row(t(n), t(d), `<button type="button" class="btn" data-url="${esc(link(k))}">${DISCORD}${esc(t('Open'))}</button>`)).join(''));
  };

  const redraw = () => { const box = document.querySelector('[data-fb]'); if (box) box.innerHTML = form(); };
  const say = (msg, bad) => { const s = document.querySelector('.bt-go .status'); if (s) { s.textContent = msg; s.className = 'status' + (bad ? ' bad' : ''); } };

  async function send() {
    const f = fb();
    if (f.busy) return;
    if (f.text.replace(/\s/g, '').length < (f.kind === 'bug' ? 12 : 3)) { say(t('Tell us a little more: what you did and what happened.'), true); const ta = document.querySelector('.bt-text'); if (ta) ta.focus(); return; }
    const u = S.account && S.account.signedIn && S.account.user;
    const contact = [f.contact.trim(), u ? `account: ${u.username}` : ''].filter(Boolean).join(' · ');
    f.busy = true; redraw();
    try {
      await invoke('beta_feedback', { kind: f.kind, area: f.area, mood: f.mood || null, message: f.text, contact: contact || null, attach: !!f.attach, game: f.game || null });
      Object.assign(f, { text: '', mood: '', game: '', sent: true, busy: false });
      redraw();
      toast(t('Sent. Thanks for testing.'));
    } catch (e) {
      f.busy = false; redraw();
      say(t(String(e && e.message ? e.message : e)), true);
    }
  }

  async function notes() {
    let v = null;
    try { v = await invoke('launcher_notes'); } catch (e) { /* offline */ }
    if (!v || !v.notes) return toast(t('The notes couldn\'t be loaded. Check your connection.'), true);
    modal(`<div class="whatsnew"><h2>${esc(tf('What\'s new in RecompLauncher {version}', { version: verName(v.version || S.version) }))}</h2><div class="md">${md(v.notes)}</div>
      <div class="acts"><button type="button" class="btn main" data-x="no">${esc(t('Got it'))}</button></div></div>`, () => {}, t('What\'s new'));
  }

  document.addEventListener('click', (e) => {
    const b = e.target.closest('[data-b]');
    if (!b || !b.closest('.set-body, .beta-hero')) return;
    const f = fb(), k = b.dataset.b;
    if (k === 'kind' || k === 'mood') {
      f[k] = k === 'mood' && f.mood === b.dataset.k ? '' : b.dataset.k;
      for (const x of b.parentElement.children) x.setAttribute('aria-checked', String(x.dataset.k === f[k]));
      if (k === 'kind') { const ta = document.querySelector('.bt-text'); if (ta) ta.placeholder = f.kind === 'bug' ? t('What you did, what you expected, and what happened instead.') : f.kind === 'idea' ? t('What would you add or change, and why?') : t('What you like, what feels off, anything at all.'); }
    } else if (k === 'attach') {
      f.attach = !f.attach; b.setAttribute('aria-checked', String(f.attach));
    } else if (k === 'send') send();
    else if (k === 'again') { f.sent = false; redraw(); const ta = document.querySelector('.bt-text'); if (ta) ta.focus(); }
    else if (k === 'notes') notes();
    else if (k === 'toreport') setSection('report');
  });
  document.addEventListener('input', (e) => {
    const el = e.target.closest && e.target.closest('[data-b="text"], [data-b="contact"]');
    if (el) fb()[el.dataset.b] = el.value;
  });
  document.addEventListener('change', (e) => {
    const el = e.target.closest && e.target.closest('select[data-b="area"], select[data-b="game"]');
    if (el) fb()[el.dataset.b] = el.value;
  });
})();
