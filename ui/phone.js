// The phone layout: the Android launcher without a controller. An app made for fingers, upright or
// sideways: pages that scroll, rows that swipe, a tab bar at the bottom and nothing that needs a
// button. With a controller it's Big Picture mode (tv.js): pressing a button on a controller
// switches to it, and when the last controller goes, it comes back here.
(function () {
  const root = document.getElementById('phone');
  if (!root) return;
  const P = { on: false, tab: 'home', game: '', q: '', back: [] };
  const TABS = [['home', N_('Home')], ['browse', N_('Catalog')], ['library', N_('Library')], ['settings', N_('Settings')]];
  const ICON = {
    home: '<path d="M3 11 12 4l9 7v9a1 1 0 0 1-1 1h-5v-6H9v6H4a1 1 0 0 1-1-1z"/>',
    browse: '<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/>',
    library: '<rect x="3" y="4" width="7" height="16" rx="1.5"/><rect x="14" y="4" width="7" height="16" rx="1.5"/>',
    settings: '<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.6 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.6-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z"/>',
    back: '<path d="M15 5l-7 7 7 7"/>',
    pad: '<path d="M6 9h4M8 7v4M15 10h.01M18 8h.01"/><path d="M17.3 5H6.7a4 4 0 0 0-3.9 3.2L1.6 14a3 3 0 0 0 5.3 2.4L8.5 14h7l1.6 2.4a3 3 0 0 0 5.3-2.4l-1.2-5.8A4 4 0 0 0 17.3 5z"/>',
  };
  const icon = (k, s = 24) => `<svg width="${s}" height="${s}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${ICON[k]}</svg>`;
  const pool = () => visible().filter((a) => forOS(a) && !a.soon);
  const pct = (j) => (j && j.total ? Math.round((j.done / j.total) * 100) : 0);
  const cover = (a, w = 360) => (a && a.cover ? media(thumbOf(a.cover, w)) : '');

  // ---------- pieces ----------
  function card(a) {
    const st = status(a), j = S.jobs[a.id];
    const badge = st === 'update' ? t('Update') : st === 'installed' ? t('Installed') : st === 'running' ? t('Playing') : '';
    return `<button type="button" class="ph-card" data-game="${esc(a.id)}" aria-label="${esc(a.name)}">
      <span class="ph-cover"><span class="ph-ph">${esc(a.name)}</span>${cover(a) ? `<img src="${esc(cover(a))}" alt="" loading="lazy" decoding="async">` : ''}${badge ? `<span class="ph-badge">${esc(badge)}</span>` : ''}${j ? `<span class="ph-bar"><i style="width:${pct(j)}%"></i></span>` : ''}</span>
      <span class="ph-name">${esc(a.name)}</span></button>`;
  }
  const row = (title, list) => (list.length ? `<section class="ph-sec"><h2>${esc(title)}</h2><div class="ph-row">${list.map(card).join('')}</div></section>` : '');
  const grid = (list) => `<div class="ph-grid">${list.map(card).join('')}</div>`;
  const empty = (msg) => `<p class="ph-empty">${esc(msg)}</p>`;

  // Android while it's for testers only: why the list is empty, and Sign In when that's the fix.
  function gate() {
    if (!S.listGate) return '';
    const signed = S.account && S.account.signedIn;
    return `<div class="ph-box"><b>${esc(t('Android games'))}</b><p>${esc(t(S.listGate))}</p>${signed ? '' : `<button type="button" class="ph-btn main" data-act="signin">${esc(t('Sign in'))}</button>`}</div>`;
  }

  // ---------- pages ----------
  function pageHome() {
    const all = pool();
    const mine = Object.values(S.library).sort((x, y) => (y.last_played || 0) - (x.last_played || 0) || (y.installed_at || 0) - (x.installed_at || 0)).map(libApp);
    const body = gate()
      + row(t('Continue playing'), mine.slice(0, 14))
      + row(t('Recently updated'), recentlyUpdated(all).slice(0, 14))
      + row(t('Just added'), [...all].sort(byNew).slice(0, 14))
      + row(t('Most played'), [...all].sort(byDl).slice(0, 14))
      + (all.length ? `<section class="ph-sec"><h2>${esc(t('All Games'))}</h2>${grid([...all].sort((x, y) => x.name.localeCompare(y.name)))}</section>` : '');
    return body || empty(S.offline ? t("Can't reach recomplauncher.com right now.") : t('Loading…'));
  }
  function pageBrowse() {
    const q = fold(P.q.trim());
    const list = pool().filter((a) => !q || fold(`${a.name} ${a.game || ''} ${a.author || ''}`).includes(q)).sort((x, y) => x.name.localeCompare(y.name));
    return `<label class="ph-search">${icon('browse', 20)}<input type="search" data-q value="${esc(P.q)}" placeholder="${esc(t('Search games'))}" autocomplete="off" enterkeyhint="search"></label>
      <div data-results>${list.length ? grid(list) : empty(q ? t('Nothing matches.') : t('No games yet'))}</div>`;
  }
  function pageLibrary() {
    const mine = Object.keys(S.library).map((id) => byId(id) || libApp(S.library[id])).filter(Boolean).sort((x, y) => x.name.localeCompare(y.name));
    if (!mine.length) return gate() + empty(t('Nothing installed yet'));
    return `<div class="ph-list">${mine.map((a) => `<div class="ph-item"><button type="button" class="ph-thumb" data-game="${esc(a.id)}" aria-label="${esc(a.name)}">${cover(a, 200) ? `<img src="${esc(cover(a, 200))}" alt="" loading="lazy">` : ''}</button>
      <button type="button" class="ph-itemk" data-game="${esc(a.id)}"><b>${esc(a.name)}</b><span>${esc((S.library[a.id] || {}).version || '')}</span></button>${mainBtn(a, true)}</div>`).join('')}</div>`;
  }
  function pageSettings() {
    const acc = S.account || {};
    const user = acc.signedIn && acc.user ? acc.user.username : '';
    return `<div class="ph-set">
      <section><h2>${esc(t('Account'))}</h2><div class="ph-setrow"><span>${esc(user ? tf('Signed in as {name}.', { name: user }) : t('Not signed in.'))}</span><button type="button" class="ph-btn" data-act="${user ? 'signout' : 'signin'}">${esc(user ? t('Sign out') : t('Sign in'))}</button></div></section>
      <section><h2>${esc(t('Big Picture mode'))}</h2><div class="ph-setrow"><span>${esc(t('For a controller. It opens by itself when you press a button on one.'))}</span><button type="button" class="ph-btn" data-act="tv">${icon('pad', 20)}${esc(t('Open'))}</button></div></section>
      <section><h2>${esc(t('About'))}</h2><div class="ph-setrow"><span>RecompLauncher ${esc(S.version || '')}</span></div></section>
    </div>`;
  }
  // The main button for a game, as on PC: Install, the download's progress, Update, Play.
  function mainBtn(a, small) {
    const st = status(a), j = S.jobs[a.id], cls = `ph-btn${small ? ' sm' : ''}`;
    switch (st) {
      case 'busy': return `<button type="button" class="${cls} prog" data-act="cancel" data-id="${esc(a.id)}"><i style="width:${pct(j)}%"></i><span>${esc(j && j.phase === 'download' && j.total ? `${pct(j)}%` : t('Installing…'))}</span></button>`;
      case 'update': return `<button type="button" class="${cls} main" data-act="update" data-id="${esc(a.id)}">${esc(t('Update'))}</button>`;
      case 'installed': case 'running': return `<button type="button" class="${cls} main" data-act="play" data-id="${esc(a.id)}">${esc(t('Play'))}</button>`;
      case 'uninstalling': return `<button type="button" class="${cls}" disabled>${esc(t('Uninstalling…'))}</button>`;
      case 'paused': return `<button type="button" class="${cls} main" data-act="resume" data-id="${esc(a.id)}">${esc(t('Resume'))}</button>`;
      case 'settingup': return `<button type="button" class="${cls}" disabled>${esc(t('Setting up'))}</button>`;
      case 'get': return `<button type="button" class="${cls} main" data-act="install" data-id="${esc(a.id)}">${esc(t('Install'))}</button>`;
      case 'soon': return `<button type="button" class="${cls}" disabled>${esc(t('Coming soon'))}</button>`;
      default: return `<button type="button" class="${cls}" disabled>${esc(tf('No {os} version yet', { os: OS_NAME[S.os] }))}</button>`;
    }
  }
  function pageGame() {
    const a = byId(P.game) || (S.library[P.game] && libApp(S.library[P.game]));
    if (!a) return empty(t('This recomp isn\'t listed anymore.'));
    const hero = a.hero || a.cover;
    const g = S.library[a.id];
    const game = a.about && a.about.summary ? a.about.summary : '';
    const recomp = a.blurb || notesLine(a.description);
    const facts = [(devOf(a) || {}).name || a.author || '', a.version ? tf('Version {v}', { v: window.verText ? window.verText(a.version) : a.version }) : '', a.android_size ? fmtB(a.android_size) : ''].filter(Boolean);
    return `<div class="ph-hero">${hero ? `<img src="${esc(media(hero))}" alt="" decoding="async">` : ''}</div>
      <div class="ph-game">
        <h1>${esc(a.name)}</h1>
        ${facts.length ? `<p class="ph-facts">${facts.map(esc).join(' · ')}</p>` : ''}
        <div class="ph-acts">${mainBtn(a)}${g ? `<button type="button" class="ph-btn" data-act="uninstall" data-id="${esc(a.id)}">${esc(t('Uninstall'))}</button>` : ''}</div>
        ${recomp ? `<section><h2>${esc(t('About this recomp'))}</h2><p class="ph-about" dir="auto">${esc(recomp)}</p></section>` : ''}
        ${game ? `<section><h2>${esc(t('About the game'))}</h2><p class="ph-about" dir="auto">${esc(game)}</p></section>` : ''}
      </div>`;
  }

  // ---------- drawing ----------
  function draw(keep) {
    if (!P.on) return;
    const page = P.game ? 'game' : P.tab;
    const scroller = $('.ph-main', root);
    const top = keep && scroller ? scroller.scrollTop : 0;
    const title = P.game ? '' : t((TABS.find((x) => x[0] === P.tab) || TABS[0])[1]);
    const html = page === 'game' ? pageGame() : page === 'browse' ? pageBrowse() : page === 'library' ? pageLibrary() : page === 'settings' ? pageSettings() : pageHome();
    const focused = document.activeElement && document.activeElement.matches && document.activeElement.matches('[data-q]');
    root.innerHTML = `<header class="ph-top${P.game ? ' over' : ''}">${P.game ? `<button type="button" class="ph-backbtn" data-act="back" aria-label="${esc(t('Back'))}">${icon('back')}</button>` : `<h1>${esc(title)}</h1>`}</header>
      <main class="ph-main ph-page-${page}">${html}</main>
      <nav class="ph-tabs">${TABS.map(([k, n]) => `<button type="button" class="ph-tab${!P.game && P.tab === k ? ' on' : ''}" data-tab="${k}">${icon(k)}<span>${esc(t(n))}</span></button>`).join('')}</nav>`;
    const m = $('.ph-main', root);
    if (m && top) m.scrollTop = top;
    if (focused) { const q = $('[data-q]', root); if (q) { q.focus(); q.setSelectionRange(q.value.length, q.value.length); } }
  }
  let later = 0;
  const soon = () => { if (!P.on) return; clearTimeout(later); later = setTimeout(() => draw(true), 120); };

  function openGame(id) { P.back.push(P.game ? { game: P.game } : { tab: P.tab }); P.game = id; draw(); }
  function back() {
    if (!P.game) { if (P.tab !== 'home') { P.tab = 'home'; draw(); return true; } return false; }
    const b = P.back.pop() || { tab: P.tab };
    P.game = b.game || '';
    if (b.tab) P.tab = b.tab;
    draw();
    return true;
  }

  // ---------- taps ----------
  root.addEventListener('click', async (e) => {
    const tab = e.target.closest('[data-tab]');
    if (tab) { P.game = ''; P.back = []; P.tab = tab.dataset.tab; return draw(); }
    const gm = e.target.closest('[data-game]');
    if (gm) return openGame(gm.dataset.game);
    const b = e.target.closest('[data-act]');
    if (!b) return;
    const id = b.dataset.id;
    switch (b.dataset.act) {
      case 'back': return back();
      case 'install': await startInstall(id); return draw(true);
      case 'update': await updateGame(id); return draw(true);
      case 'play': return playGame(id);
      case 'cancel': return cancelDownload(id);
      case 'resume': resumeDl(id); return draw(true);
      case 'uninstall':
        try { await invoke('uninstall', { id, deleteFiles: false }); } catch (err) { return fail(err); }
        try { await refreshState(); } catch (err) { /* the next look */ }
        return draw(true);
      case 'signin': return window.signIn && window.signIn(false);
      case 'signout': if (typeof signOut === 'function') await signOut(); return draw(true);
      case 'tv': return toTV();
    }
  });
  root.addEventListener('input', (e) => {
    if (!e.target.matches('[data-q]')) return;
    P.q = e.target.value;
    const q = fold(P.q.trim());
    const list = pool().filter((a) => !q || fold(`${a.name} ${a.game || ''} ${a.author || ''}`).includes(q)).sort((x, y) => x.name.localeCompare(y.name));
    const r = $('[data-results]', root);
    if (r) r.innerHTML = list.length ? grid(list) : empty(q ? t('Nothing matches.') : t('No games yet'));
  });

  // ---------- in and out ----------
  async function orient(landscape) {
    try { await invoke('android_orientation', { landscape }); } catch (e) { /* stays as it is */ }
  }
  function enter() {
    if (P.on) return;
    P.on = true;
    document.body.classList.add('phone-on');
    root.hidden = false;
    orient(false);
    if (window.hookAndroidBack) window.hookAndroidBack();
    draw();
  }
  function leave() {
    P.on = false;
    document.body.classList.remove('phone-on');
    root.hidden = true;
    root.innerHTML = '';
  }
  // Big Picture mode, for a controller (or picked in Settings). It closes this layout as it opens.
  const toTV = () => window.startTV && window.startTV();
  window.startPhone = enter;
  window.phoneLeave = leave;
  window.phoneOn = () => P.on;
  window.phoneBack = back;

  // Keep the pages current while things happen elsewhere.
  T.event.listen('install', () => soon());
  window.phoneRefresh = soon;
})();
