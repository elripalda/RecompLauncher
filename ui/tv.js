'use strict';
// TV mode: full screen, for a controller. Shares the desktop window's data (S) and helpers from
// app.js, and draws its own pages on a stage scaled to the screen (three sizes).
//
// Every page has the same frame: the bar at the top, the hints at the bottom, and the region in
// between where the page's content lives and scrolls. Nothing is placed outside the region, so
// nothing can run into the bar, the hints or each other, whatever the size or the content.
//
// Controllers arrive as `pad` events from the core ({b, kind}); the keyboard works too (arrows,
// Enter, Esc, Q and E for the bumpers, X and Y).
(() => {
  const root = $('#tv');
  const TV = { on: false, view: 'home', arg: null, back: [], hero: null, row: {}, focusKey: null, genre: '', shot: null, sheet: null };
  const TABS = [['home', N_('Home')], ['library', N_('Library')], ['discover', N_('Discover')], ['downloads', N_('Downloads')], ['settings', N_('Settings')]];
  const SX = 96; // the side margin, in stage pixels
  const MARK = '<svg viewBox="0 0 256 256" aria-hidden="true"><defs><linearGradient id="tvm" x1="0" y1="38" x2="0" y2="218" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="#34f2e8"/><stop offset="1" stop-color="#377bb6"/></linearGradient></defs><g fill="url(#tvm)" transform="translate(4 -2)"><path d="M40 40H88V168H116L64 220L12 168H40Z"/><path d="M104 40H172L216 84V116L180 152H104V116H164L180 100V92L164 76H104Z"/><path d="M132 168H184L236 220H184Z"/></g></svg>';
  const ICON = {
    cloud: '<path d="M7 18a4.5 4.5 0 0 1-.6-9 6 6 0 0 1 11.5 1.6A3.8 3.8 0 0 1 17.5 18z"/>',
    user: '<circle cx="12" cy="8.5" r="3.6"/><path d="M4.8 20a7.2 7.2 0 0 1 14.4 0"/>',
    check: '<path d="m5 12.5 4.5 4.5L19 7.5"/>',
    shot: '<rect x="3" y="6" width="18" height="14" rx="2"/><path d="M8 6l1.5-2h5L16 6"/><circle cx="12" cy="13" r="3.5"/>',
    puzzle: '<path d="M9 4h4v2.5a1.5 1.5 0 0 0 3 0V4h4v6h-2.5a1.5 1.5 0 0 0 0 3H20v7h-6v-2.5a1.5 1.5 0 0 0-3 0V20H4v-6h2.5a1.5 1.5 0 0 0 0-3H4V4z"/>',
    more: '<circle cx="5" cy="12" r="1.6" fill="currentColor"/><circle cx="12" cy="12" r="1.6" fill="currentColor"/><circle cx="19" cy="12" r="1.6" fill="currentColor"/>',
    steam: '<path d="M12 5v14M5 12h14"/>',
    desk: '<rect x="3" y="4" width="18" height="13" rx="2"/><path d="M8 21h8M12 17v4"/>',
    layers: '<path d="m12 3 9 5-9 5-9-5z"/><path d="m3 13 9 5 9-5"/>',
    history: '<path d="M4 12a8 8 0 1 0 2.3-5.6M4 4v4.5h4.5"/><path d="M12 8v4.5l3 2"/>',
    folder: '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>',
    stop: '<path d="m6 6 12 12M18 6 6 18"/>',
    gear: '<circle cx="12" cy="12" r="3"/><path d="M12 3v2.5M12 18.5V21M3 12h2.5M18.5 12H21M5.6 5.6l1.8 1.8M16.6 16.6l1.8 1.8M5.6 18.4l1.8-1.8M16.6 7.4l1.8-1.8"/>',
    chevL: '<path d="m15 5-7 7 7 7"/>',
    chevR: '<path d="m9 5 7 7-7 7"/>',
    disc: '<circle cx="12" cy="12" r="9"/><circle cx="12" cy="12" r="2.5"/>',
    refresh: '<path d="M20 11a8 8 0 1 0-2.3 5.7M20 4v7h-7"/>',
    disk: '<rect x="3" y="7" width="18" height="10" rx="2"/><circle cx="17" cy="12" r="1"/>',
    file: '<path d="M6 3h8l4 4v14H6z"/><path d="M14 3v4h4"/>',
    bin: '<path d="M5 7h14M10 7V5h4v2M7 7l1 13h8l1-13"/>',
    pad: '<path d="M7.5 7h9a5 5 0 0 1 4.8 6.4l-1 3.4a2.6 2.6 0 0 1-4.4 1L14 16h-4l-1.9 1.8a2.6 2.6 0 0 1-4.4-1l-1-3.4A5 5 0 0 1 7.5 7z"/><path d="M8 10.5v3M6.5 12h3"/><circle cx="16" cy="11" r=".9" fill="currentColor"/><circle cx="17.6" cy="13" r=".9" fill="currentColor"/>',
  };
  const svg = (k, s = 26) => `<svg width="${s}" height="${s}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${ICON[k]}</svg>`;

  // Button icons for the controller in use: Xbox letters, or PlayStation shapes.
  const GLYPH_NAME = { a: 'A', b: 'B', x: 'X', y: 'Y', lb: 'LB', rb: 'RB', view: N_('View'), menu: N_('Menu') };
  const PS_NAME = { a: N_('Cross'), b: N_('Circle'), x: N_('Square'), y: N_('Triangle'), lb: 'L1', rb: 'R1', view: N_('Create'), menu: N_('Options') };
  const glyph = (n, cls = '') => {
    const ps = S.padKind === 'ps';
    return `<i class="glyph g-${n} ${cls}" style="--m:url(img/pad/${ps ? 'ps' : 'xbox'}-${n}.${ps ? 'png' : 'svg'})" role="img" aria-label="${esc(t((ps ? PS_NAME : GLYPH_NAME)[n] || n))}"></i>`;
  };

  // ---------- sounds ----------
  // Start-up, moving, going back and changing section are RecompStore's. Picking something and
  // opening a game are soft, short tones made here.
  const SFX = {};
  let ctx = null;
  function tone(kind) {
    try {
      ctx = ctx || new (window.AudioContext || window.webkitAudioContext)();
      if (ctx.state === 'suspended') ctx.resume();
      const t0 = ctx.currentTime, out = ctx.createGain(), lp = ctx.createBiquadFilter();
      lp.type = 'lowpass'; lp.frequency.value = 2600;
      out.gain.value = 0.9;
      lp.connect(out); out.connect(ctx.destination);
      const note = (f, at, len, vol, f2) => {
        const o = ctx.createOscillator(), g = ctx.createGain();
        o.type = 'sine';
        o.frequency.setValueAtTime(f, t0 + at);
        if (f2) o.frequency.exponentialRampToValueAtTime(f2, t0 + at + len * 0.7);
        g.gain.setValueAtTime(0.0001, t0 + at);
        g.gain.exponentialRampToValueAtTime(vol, t0 + at + 0.008);
        g.gain.exponentialRampToValueAtTime(0.0001, t0 + at + len);
        o.connect(g); g.connect(lp);
        o.start(t0 + at); o.stop(t0 + at + len + 0.02);
      };
      if (kind === 'select') { note(740, 0, 0.11, 0.07); note(1110, 0.045, 0.14, 0.055); }
      else if (kind === 'open') { note(330, 0, 0.26, 0.05, 520); note(990, 0.03, 0.2, 0.03, 1480); }
    } catch (e) { /* no sound is fine */ }
  }
  const sfx = (n) => {
    if (S.settings.sounds === false) return;
    if (n === 'select' || n === 'open') return tone(n);
    try {
      const a = SFX[n] || (SFX[n] = new Audio(`sfx/${n}.webm`));
      a.volume = n === 'boot' ? 0.7 : n === 'move' ? 0.3 : 0.4;
      a.currentTime = 0;
      a.play().catch(() => {});
    } catch (e) { /* no sound is fine */ }
  };

  // ---------- the stage ----------
  // A bigger stage makes everything smaller on screen.
  const SIZES = { small: [2400, 1350], medium: [2133, 1200], large: [1920, 1080] };
  function stageSize() {
    let k = S.settings.tv_size;
    if (!SIZES[k]) { const w = screen.width * (devicePixelRatio || 1); k = w >= 3000 ? 'small' : w >= 2200 ? 'medium' : 'large'; }
    return SIZES[k];
  }
  const stage = () => $('.tv-stage', root);
  function fit() {
    const st = stage();
    if (!st) return;
    const [w, h] = stageSize();
    // Scaled to fit the 16:9 size, then stretched to the screen's own shape, so a 16:10 Steam Deck
    // or an ultrawide is filled edge to edge instead of getting bands.
    const k = Math.min(innerWidth / w, innerHeight / h);
    const W = Math.round(innerWidth / k), H = Math.round(innerHeight / k);
    st.style.width = W + 'px';
    st.style.height = H + 'px';
    st.style.setProperty('--H', H + 'px');
    st.style.transform = `translate(-50%, -50%) scale(${k})`;
    TV.k = k;
    TV.w = W;
  }
  addEventListener('resize', () => { fit(); if (TV.on && !$('.tv-boot', root)) draw(true); });

  // Names never wrap or get cut off: smaller first, then the short name, then a glide when focused.
  function fitName(el, floor) {
    if (el.dataset.full) el.textContent = el.dataset.full; else el.dataset.full = el.textContent;
    el.style.fontSize = '';
    el.classList.remove('long');
    const w = el.clientWidth;
    if (!w) return;
    const base = parseFloat(getComputedStyle(el).fontSize);
    const shrink = (f) => { let size = parseFloat(el.style.fontSize) || base; while (el.scrollWidth > w + 0.5 && size > f) { size -= 1; el.style.fontSize = size + 'px'; } };
    shrink(Math.max(floor, base * 0.72));
    const short = shortName(el.dataset.full);
    if (el.scrollWidth > w + 0.5 && short !== el.dataset.full) { el.textContent = short; el.style.fontSize = ''; shrink(Math.max(floor, base * 0.72)); }
    shrink(floor);
    const over = el.scrollWidth - w;
    if (over > 0.5) {
      el.classList.add('long');
      el.innerHTML = `<span class="glide">${esc(el.textContent)}</span>`;
      el.style.setProperty('--shift', -Math.ceil(over + 8) + 'px');
      el.style.setProperty('--glide', Math.max(1.2, over / 60).toFixed(2) + 's');
    }
  }
  function fitAll() {
    for (const el of $$('.tv-h1', root)) fitName(el, 48);
    for (const el of $$('.tv-card .cap b', root)) fitName(el, 16);
    for (const el of $$('.tv-name', root)) fitName(el, 15);
  }

  // Grids: as many columns as fit, never ending on a lone tile.
  function evenCols(n, width, min, gap) {
    const fits = Math.max(1, Math.floor((width + gap) / (min + gap)));
    if (n <= fits) return fits;
    return [fits, fits + 1, fits - 1, fits - 2].find((c) => c >= 2 && n % c !== 1) || fits;
  }

  async function enter() {
    if (TV.on) return;
    TV.on = true;
    TV.enteredAt = performance.now();
    document.body.classList.add('tv-on');
    root.hidden = false;
    try { await W.setFullscreen(true); } catch (e) { /* the window can stay as it is */ }
    boot();
  }
  async function leave() {
    TV.on = false;
    root.hidden = true;
    root.innerHTML = '';
    document.body.classList.remove('tv-on');
    try { await W.setFullscreen(false); } catch (e) { /* fine */ }
    render(true);
  }
  window.enterTV = enter;
  window.startTV = enter;

  function boot() {
    sfx('boot');
    root.innerHTML = `<div class="tv-stage"><div class="tv-boot"><div class="aurora"><i></i><i></i><i></i></div>
      <div class="logo"><svg width="236" height="236" viewBox="0 0 256 256" aria-hidden="true"><defs><linearGradient id="tvb" x1="0" y1="38" x2="0" y2="218" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="#34f2e8"/><stop offset="1" stop-color="#377bb6"/></linearGradient></defs><g fill="url(#tvb)" transform="translate(4 -2)"><path d="M40 40H88V168H116L64 220L12 168H40Z"/><path d="M104 40H172L216 84V116L180 152H104V116H164L180 100V92L164 76H104Z"/><path d="M132 168H184L236 220H184Z"/></g></svg>
        <img class="word" src="img/wordmark.svg" alt="RecompLauncher"></div>
      <div class="load"><div>${esc(t('Loading your library'))}<span><i></i><i></i><i></i></span></div><div class="bar"><i></i></div></div>
    </div></div>`;
    fit();
    if (!S.news) loadNews();
    setTimeout(() => {
      const b = $('.tv-boot', root);
      if (!b || !TV.on) return;
      b.classList.add('out');
      setTimeout(() => { if (!TV.on) return; TV.view = 'home'; TV.back = []; TV.enter = 'boot'; draw(); }, 420);
    }, 3100);
  }

  // ---------- pieces ----------
  const heroOf = (a) => (a ? media(a.hero || a.cover) : '');
  const installed = () => Object.values(S.library).sort((x, y) => (y.last_played || 0) - (x.last_played || 0) || (y.installed_at || 0) - (x.installed_at || 0)).map((g) => ({ g, a: byId(g.id) || { id: g.id, name: g.name, systems: [S.os] } }));
  const pool = () => visible().filter((a) => forOS(a) && !a.soon);
  const timeLine = (g) => (!g ? '' : !g.last_played ? t('Not played yet') : `${ago(g.last_played).replace(/^./, (c) => c.toUpperCase())} · ${playedFor(g)}`);

  const htmlIn = (text, v) => esc(text).replace(/\{(\w+)\}/g, (m, k) => (k in v ? v[k] : m));
  // A wide card (Continue playing, What's new).
  function card(a, key, sub) {
    const g = S.library[a.id];
    return `<button type="button" class="tv-card" data-f data-key="${key}" data-hero="${esc(a.id)}" data-go-game="${esc(a.id)}">
      <span class="ph">${esc(a.name)}</span>${a.hero || a.cover ? `<img src="${esc(heroOf(a))}" alt="">` : ''}
      ${S.running.has(a.id) ? `<span class="tv-badge">${esc(t('Playing'))}</span>` : S.updates[a.id] ? `<span class="tv-badge">${esc(t('Update'))}</span>` : ''}
      <span class="cap"><b>${esc(a.name)}</b><span>${esc(sub != null ? sub : g ? timeLine(g) : a.game || '')}</span></span></button>`;
  }

  // A cover, with its name under it.
  function poster(a, key, sub) {
    const st = status(a), j = S.jobs[a.id];
    const badge = st === 'update' ? `<span class="tv-badge">${esc(t('Update'))}</span>` : st === 'running' ? `<span class="tv-badge">${esc(t('Playing'))}</span>` : st === 'soon' ? `<span class="tv-badge soon">${esc(t('Soon'))}</span>` : st === 'installed' && TV.view !== 'library' ? `<span class="tv-badge left">${esc(t('Installed'))}</span>` : '';
    return `<div class="tv-cell">${a.cover ? `<span class="aura" style="background-image:url('${cssUrl(media(a.cover))}')"></span>` : ''}<button type="button" class="tv-poster" data-f data-key="${key}" data-hero="${esc(a.id)}" data-go-game="${esc(a.id)}" aria-label="${esc(a.name)}">
      <span class="ph">${esc(a.name)}</span>${a.cover ? `<img src="${esc(media(a.cover))}" alt="" loading="lazy">` : ''}${badge}${TV.view !== 'library' && !j && verGroup(a) ? `<span class="tv-vers">${svg('layers', 18)}${fmtNum(verGroup(a).list.length)}</span>` : ''}${j ? `<span class="tv-pbar"><i data-prog-bar="${esc(a.id)}" style="width:${pct(j)}%"></i></span>` : ''}</button>
      <span class="tv-name">${esc(a.name)}${(devOf(a) || {}).verified ? TICK(18) : ''}</span>${sub ? `<span class="tv-sub">${esc(sub)}</span>` : ''}</div>`;
  }

  const btn = (key, act, label, { cls = '', icon = '', id = '', g = '' } = {}) => `<button type="button" class="tv-btn ${cls}" data-f data-key="${key}" data-tvact="${act}" data-id="${esc(id)}">${g ? glyph(g, 'tv-keyg') : icon ? svg(icon) : ''}<span>${label}</span></button>`;

  // The main button: Play, Install, Update, a download's progress, or Back to the game.
  function mainButton(a, key) {
    const st = status(a), id = a.id;
    switch (st) {
      case 'running': return btn(key, 'resume', esc(t('Back to the game')), { cls: 'main', id, g: 'a' });
      case 'settingup': return `<button type="button" class="tv-btn" data-f data-key="${key}" disabled><span>${esc(t('Its setup is open'))}</span></button>`;
      case 'busy': return `<button type="button" class="tv-btn wide" data-f data-key="${key}" data-tvact="cancel" data-id="${esc(id)}">${glyph('a', 'tv-keyg')}${progress(id)}</button>`;
      case 'update': return btn(key, 'install', esc(t('Update now')), { cls: 'main', id, g: 'a' });
      case 'installed': { const su = S.setup[id]; return S.easy.has(id) && su && su !== 'loading' && su.needs && !su.copy ? btn(key, 'setup', esc(t('Set up')), { cls: 'main', id, g: 'a' }) : btn(key, 'play', esc(t('Play')), { cls: 'main', id, g: 'a' }); }
      case 'soon': return `<button type="button" class="tv-btn" data-f data-key="${key}" disabled><span>${esc(t('Coming soon'))}</span></button>`;
      case 'nobuild': return `<button type="button" class="tv-btn" data-f data-key="${key}" disabled><span>${esc(tf('No {os} version yet', { os: OS_NAME[S.os] }))}</span></button>`;
      default: return btn(key, 'install', esc(t('Install')) + (a.size ? ` <small>${esc(a.size)}</small>` : ''), { cls: 'main', id, g: 'a' });
    }
  }

  const updatesReady = () => Object.keys(S.updates || {}).filter((id) => S.library[id] && !S.jobs[id]).length + (S.launcherUpd ? 1 : 0);
  const tabOf = () => (['game', 'shots', 'mods', 'gsettings'].includes(TV.view) ? TV.from || 'home' : TV.view);
  const clock = () => new Date().toLocaleTimeString(curLocale(), { hour: 'numeric', minute: '2-digit' });
  const bar = () => `<header class="tv-bar"><div class="tv-brand">${MARK}<img class="tv-wm" src="img/wordmark.svg" alt="RecompLauncher"></div>
    <nav class="tv-nav" aria-label="${esc(t('Sections'))}">${glyph('lb', 'tv-bumpg')}${TABS.map(([k, n]) => `<button type="button" class="tv-tab${tabOf() === k ? ' on' : ''}" data-f data-key="tab:${k}" data-tab="${k}">${esc(t(n))}${k === 'downloads' && updatesReady() ? `<b class="tv-tab-n">${updatesReady()}</b>` : ''}</button>`).join('')}${glyph('rb', 'tv-bumpg')}</nav>
    <div class="tv-status">${(S.pads || []).length || S.padKind ? `<span class="tv-padico" title="${esc(t('Controller'))}">${svg('pad', 30)}</span>` : ''}<span class="tv-clock" id="tv-clock">${clock()}</span></div></header>`;
  // The game page has a Back pill instead of the sections.
  const gameBar = () => `<header class="tv-bar"><button type="button" class="tv-back" data-f data-key="back" data-tvact="back">${glyph('b', 'tv-keyg')}<span>${esc(t('Back'))}</span></button><span></span>
    <div class="tv-status"><span class="tv-clock" id="tv-clock">${clock()}</span></div></header>`;

  function hints(list) {
    const g = (k) => k.toLowerCase().split('+').map((n) => glyph(n)).join('<em>+</em>');
    const playing = S.running.size > 0;
    return `<footer class="tv-hints">${list.map(([k, l]) => `<span>${g(k)}${esc(l)}</span>`).join('')}${playing ? `<span class="end">${glyph('view')}${esc(t('Hold for the quick menu'))}</span>` : ''}</footer>`;
  }

  const region = (inner, cls = '') => `<main class="tv-region ${cls}"><div class="tv-scroll" id="tv-scroll">${inner}</div></main>`;
  const backdrop = (a, cls = '') => `<div class="tv-full ${cls}" id="tv-bg">${a && (a.hero || a.cover) ? `<img src="${esc(heroOf(a))}" alt="">` : '<div class="tv-glow"></div>'}</div>`;
  // `raw`: the title is HTML already (with a tag after it).
  const pageHead = (title, sub, raw) => `<div class="tv-head"><h2>${raw ? title : esc(title)}</h2>${sub ? `<p>${esc(sub)}</p>` : ''}</div>`;
  const row = (name, title, items) => `<section class="tv-sec" data-sec="${name}"><h3>${esc(title)}</h3><div class="tv-rowview"><div class="tv-row" data-row="${name}">${items}</div></div></section>`;

  // ---------- pages ----------
  function pageHome() {
    const mine = installed();
    const cont = mine.map((x) => x.a);
    const first = cont.length ? cont : oneEach(pool().sort(byDl)).slice(0, 10);
    const sel = byId(TV.hero) || first[0];
    TV.hero = sel ? sel.id : null;
    const rows = [];
    rows.push(row('cont', cont.length ? t('Continue playing') : t('Most played recomps'), first.map((a) => card(a, 'c:' + a.id)).join('')));
    const owned = oneEach(Object.keys(S.owned).filter((id) => !S.library[id]).map(byId).filter((a) => a && !a.soon && forOS(a) && !(S.settings.hide_ai && isAI(a)) && !haveAny(a)).sort(byDl));
    if (owned.length) rows.push(row('own', t('Found in your folders'), owned.map((a) => poster(a, 'o:' + a.id, t('You have it'))).join('')));
    const disc = oneEach(pool().filter((a) => S.easy.has(a.id) && !haveAny(a)));
    if (disc.length >= 2) rows.push(row('disc', t('Drop in your disc'), disc.map((a) => poster(a, 'e:' + a.id, t('Easy Setup'))).join('')));
    const news = (S.news || []).filter((x) => byId(x.id) || S.library[x.id]).slice(0, 8);
    if (news.length) rows.push(row('news', t('What\'s new for your games'), news.map((x) => card(byId(x.id) || { id: x.id, name: x.name }, `w:${x.id}:${x.tag}`, `${x.title.startsWith(x.name) ? x.tag : x.title} · ${date(x.date)}`)).join('')));
    const upd = recentlyUpdated(pool()).slice(0, 14);
    if (upd.length) rows.push(row('upd', t('Recently updated'), upd.map((a) => poster(a, 'u:' + a.id, tf('Updated {when}', { when: ago(Date.parse(a.released) / 1000) }))).join('')));
    rows.push(row('new', t('Just added'), oneEach(pool().sort(byNew), true).slice(0, 14).map((a) => poster(a, 'n:' + a.id)).join('')));
    rows.push(row('top', t('Most played'), oneEach(pool().sort(byDl)).slice(0, 14).map((a) => poster(a, 't:' + a.id)).join('')));
    const g = sel && S.library[sel.id];
    return `${backdrop(sel)}${bar()}
      ${region(`<div class="tv-hero" id="tv-hero">${sel ? heroText(sel, g) : `<h1 class="tv-h1">${esc(t('Welcome'))}</h1>`}</div>${rows.join('')}`, 'home')}
      ${hints([['A', t('Select')], ['Y', t('Game page')], ['LB+RB', t('Sections')]])}`;
  }

  function heroText(a, g) {
    const kick = S.running.has(a.id) ? t('Playing now') : S.updates[a.id] ? t('Update ready') : g ? (g.last_played ? tf('Played {when}', { when: ago(g.last_played) }) : t('In your library')) : a.featured ? t('Featured') : t('Most played');
    const meta = [a.game ? '' : originName(a.origin), g ? (g.played_secs ? tf('{time} played', { time: fmtTime(g.played_secs) }) : '') : a.downloads ? tf('{n} downloads', { n: fmtN(a.downloads) }) : ''].filter(Boolean).join(' · ');
    return `<span class="tv-kick">${esc(kick)}</span><h1 class="tv-h1">${esc(a.name)}</h1>${a.game ? `<p class="tv-based">${DISC_S.replace(/14/g, '24')}<span>${htmlIn(t('Based on {game}'), { game: `<b>${esc(a.game)}</b>` })}${a.origin ? ` · ${esc(originName(a.origin))}` : ''}</span></p>` : ''}${meta ? `<p class="tv-meta">${esc(meta)}</p>` : ''}
      <div class="tv-acts">${mainButton(a, 'hero-main')}${btn('hero-page', 'page', esc(t('Game page')), { id: a.id, g: 'y' })}</div>`;
  }

  function grid(list, key, opts = {}) {
    const w = TV.w || stageSize()[0];
    const cols = evenCols(list.length, w - SX * 2, opts.min || 210, opts.gap || 30);
    return `<div class="tv-grid ${opts.cls || ''}" style="grid-template-columns:repeat(${cols},minmax(0,1fr))">${list.map((a, i) => (opts.item ? opts.item(a, i) : poster(a, key + ':' + a.id))).join('')}</div>`;
  }

  function pageLibrary() {
    const list = installed().map((x) => x.a);
    return `${backdrop(null)}${bar()}
      ${region(`${pageHead(t('Library'), list.length ? tn(list.length, '1 game, kept up to date from each developer\'s GitHub', '{n} games, kept up to date from each developer\'s GitHub') : t('Install a recomp from Discover and it shows up here'))}
        ${list.length ? grid(list, 'L') : `<p class="tv-empty">${esc(t('Nothing installed yet.'))}</p>`}`)}
      ${hints([['A', t('Open')], ['B', t('Back')], ['LB+RB', t('Sections')]])}`;
  }

  function pageDiscover() {
    const g = TV.genre || '';
    const apps = pool();
    const genres = Object.keys(GENRES).filter((x) => apps.some((a) => a.genre === x));
    // "Easy Setup" is a chip of its own, before the genres.
    const list = oneEach(apps.filter((a) => (g === 'easy' ? S.easy.has(a.id) && !a.soon : !g || a.genre === g)).sort(byDl));
    const chips = `<div class="tv-rowview chips"><div class="tv-row tv-chips" data-row="genres"><span class="tv-chip-pill" aria-hidden="true"></span>${[['', N_('All')]].concat(apps.some((a) => S.easy.has(a.id)) ? [['easy', N_('Easy Setup')]] : [], genres.map((x) => [x, GENRES[x]])).map(([x, n]) => `<button type="button" class="tv-chip${g === x ? ' on' : ''}" data-f data-key="g:${x}" data-genre="${x}">${x === 'easy' ? DISC_S.replace(/14/g, '20') : catIcon(x)}${esc(t(n))}</button>`).join('')}</div></div>`;
    return `${backdrop(null)}${bar()}
      ${region(`${pageHead(g === 'easy' ? t('Easy Setup') : g ? t(GENRES[g]) : t('Discover'), list.length === 1 ? tf('1 recomp for {os}. You bring your own copy of each game.', { n: 1, os: OS_NAME[S.os] }) : tf('{n} recomps for {os}. You bring your own copy of each game.', { n: list.length, os: OS_NAME[S.os] }))}${chips}
        <div class="tv-gridwrap${TV.swap ? ' swap' : ''}">${list.length ? grid(list, 'D') : `<p class="tv-empty">${esc(t('Nothing here yet.'))}</p>`}</div>`)}
      ${hints([['A', t('Open')], ['Y', t('Game page')], ['LB+RB', t('Sections')]])}`;
  }

  function pageDownloads() {
    const act = Object.keys(S.jobs), rec = S.recent.filter((r) => !S.jobs[r.id]).slice(0, 6);
    const item = (id, inner, k, extra = '') => { const a = byId(id) || { id, name: (S.library[id] || {}).name || id }; return `<button type="button" class="tv-item" data-f data-key="${k}" data-go-game="${esc(id)}">${a.cover ? `<img src="${esc(media(a.cover))}" alt="">` : '<span class="ph"></span>'}<span class="k">${esc(a.name)}<small>${inner}</small></span>${extra}</button>`; };
    const ups = Object.keys(S.updates || {}).filter((id) => S.library[id] && !S.jobs[id]);
    const items = act.map((id) => item(id, '', 'd:' + id, progress(id))).concat(ups.map((id) => item(id, esc(tf('Update ready: {from} → {to}', { from: S.library[id].tag || '?', to: S.updates[id] })), 'u:' + id))).concat(rec.map((r) => item(r.id, r.phase === 'error' ? esc(cleanMsg(r.msg)) : r.phase === 'cancelled' ? esc(t('Cancelled')) : esc(t('Installed and ready to play')), 'r:' + r.id)));
    return `${backdrop(null)}${bar()}
      ${region(`${pageHead(t('Downloads'), t('Each download is checked against GitHub\'s checksum before it\'s installed.'))}
        <div class="tv-list">${items.length ? items.join('') : `<p class="tv-empty">${esc(t('Nothing downloading.'))}</p>`}</div>`)}
      ${hints([['A', t('Open')], ['B', t('Back')], ['LB+RB', t('Sections')]])}`;
  }

  // Settings in groups, with the focused one shown large on the right (what it does, and its value).
  function pageSettings() {
    const sw = (k, label, sub, on = !!S.settings[k]) => `<button type="button" class="tv-item" data-f data-key="s:${k}" data-toggle="${k}" data-sw="${on ? 1 : 0}"><span class="k">${esc(label)}<small>${esc(sub)}</small></span><span class="switch" aria-checked="${on}"></span></button>`;
    const size = { small: 'Smaller', medium: 'Medium', large: 'Larger' }[S.settings.tv_size] || 'Auto';
    const head = (n) => `<h3 class="tv-lh">${esc(n)}</h3>`;
    return `${backdrop(null)}${bar()}
      ${region(`${pageHead(t('Settings'), t('Folders, updates and the rest are in the desktop view.'))}
        <div class="tv-setwrap"><div class="tv-list tv-setlist">
          ${head(t('Account'))}
          ${S.account && S.account.signedIn
            ? `<button type="button" class="tv-item" data-f data-key="s:account" data-tvact="account" data-val="${esc(S.account.user.username)}"><span class="k">${esc(tf('Signed in as {name}', { name: S.account.user.username }))}<small>${esc(t('Cloud saves keep each game\'s saves on your account'))}</small></span><span class="tv-pill on">${esc(t('Account'))}</span></button>`
            : `<button type="button" class="tv-item" data-f data-key="s:account" data-tvact="account"><span class="k">${esc(t('Sign in'))}<small>${esc(t('Back your saves up to a RecompStore account, and play on with them on any PC'))}</small></span><span class="tv-pill on">${esc(t('Sign in'))}</span></button>`}
          ${head(t('Display'))}
          <button type="button" class="tv-item" data-f data-key="s:size" data-tvact="size" data-val="${esc(t(size))}"><span class="k">${esc(t('Size'))}<small>${esc(t('How big everything is in Big Picture mode'))}</small></span><span class="tv-seg">${[N_('Auto'), N_('Smaller'), N_('Medium'), N_('Larger')].map((n) => `<span class="${n === size ? 'on' : ''}">${esc(t(n))}</span>`).join('')}</span></button>
          <button type="button" class="tv-item" data-f data-key="s:lang" data-tvact="lang" data-val="${esc(langName())}"><span class="k">${esc(t('Language'))}<small>${esc(langName())}</small></span><span class="tv-pill on">${esc(t('Change'))}</span></button>
          ${sw('sounds', t('Sounds'), t('The start-up sound and a soft click as you move around'), S.settings.sounds !== false)}
          ${head(t('Starting up'))}
          ${sw('start_in_tv', t('Start in Big Picture mode'), t('Open full screen, ready for a controller'))}
          ${head(t('Games and updates'))}
          ${sw('notify', t('Notifications'), t('When a new recomp comes out for a game you own'))}
          ${sw('backup_before_update', t('Back up saves before updates'), t('The newest five copies of each game\'s saves'))}
          ${sw('hide_ai', t('Hide AI-assisted recomps'), t('They won\'t show in Discover'))}
          ${sw('discord', t('Discord status'), t('Show the game you\'re playing on your Discord profile'), S.settings.discord !== false)}
          ${head(t('Controller'))}
          <button type="button" class="tv-item" data-f data-key="s:buttons"><span class="k">${esc(t('Buttons'))}<small>${esc(t('What each button does'))}</small></span></button>
          ${head(t('Big Picture mode'))}
          <button type="button" class="tv-item" data-f data-key="s:leave" data-tvact="leave"><span class="k">${esc(t('Leave Big Picture mode'))}<small>${esc(t('Back to the desktop view, for mouse and keyboard'))}</small></span><span class="tv-pill on">${esc(t('Leave'))}</span></button>
        </div><aside class="tv-setpanel" id="tv-setpanel" aria-live="polite"></aside></div>`, 'top')}
      ${hints([['A', t('Change')], ['B', t('Back')], ['LB+RB', t('Sections')]])}`;
  }

  // The language picked in Settings, or the computer's when it's left to follow it.
  const langName = () => {
    const l = (S.settings.ui || {}).lang, name = (c) => (LANGS.find((x) => x[0] === c) || [c, c])[1];
    return !l || l === 'auto' ? tf('Same as your computer ({lang})', { lang: name(systemLang()) }) : name(l);
  };
  function pickLang() {
    const cur = (S.settings.ui || {}).lang || 'auto';
    const items = [['auto', S.os === 'windows' ? t('Same as Windows') : t('Same as your computer')]].concat(LANGS).map(([c, n]) => ({ label: (c === cur ? '✓ ' : '') + n, run: () => window.setLanguage && window.setLanguage(c) }));
    sheet(t('Language'), items);
    const at = items.findIndex((x) => x.label.startsWith('✓'));
    if (at > 0) focus($(`[data-sheet="${at}"]`, root), true);
  }

  const shotsOf = (id) => (S.allShots || []).filter((x) => !id || x.id === id);
  function pageShots(id) {
    const list = shotsOf(id);
    if (!S.allShots) loadAllShots().then(() => TV.on && TV.view === 'shots' && draw(true));
    const name = id ? (S.library[id] || {}).name || '' : '';
    return `${backdrop(id ? byId(id) : null, 'dim')}${gameBar()}
      ${region(`${pageHead(id ? tf('{name}: screenshots', { name }) : t('Screenshots'), list.length ? tn(list.length, '1 screenshot. Share one and paste it anywhere.', '{n} screenshots. Share one and paste it anywhere.') : t('None yet. Press View + RB while you play, or F12 on a keyboard.'))}
        ${list.length ? grid(list, 'sh', { min: 380, gap: 30, cls: 'shots', item: (x, i) => `<div class="tv-cell"><button type="button" class="tv-shot" data-f data-key="sh:${i}" data-shot-i="${i}"><img src="${esc(fileSrc(x.path))}" alt="" loading="lazy"></button><span class="tv-name">${esc(id ? when(x.time) : x.game)}</span></div>` }) : ''}`, 'top')}
      ${hints([['A', t('View')], ['X', t('Share')], ['B', t('Back')]])}`;
  }

  // A screenshot, full screen: left and right for the others, X shares it, Y deletes it, B goes back.
  function shotView(i) {
    const list = shotsOf(TV.arg);
    if (!list[i]) return;
    TV.shot = i;
    let v = $('.tv-viewer', stage());
    if (!v) { v = document.createElement('div'); v.className = 'tv-viewer'; stage().append(v); }
    const x = list[i];
    v.innerHTML = `<img src="${esc(fileSrc(x.path))}" alt="${esc(t('Screenshot'))}"><div class="tv-viewer-bar"><span><b>${esc(x.game)}</b> · ${esc(when(x.time))} · ${esc(tf('{i} of {n}', { i: i + 1, n: list.length }))}</span>
      <span class="tv-hints-inline"><span>${glyph('x')}${esc(t('Share'))}</span><span>${glyph('y')}${esc(t('Delete'))}</span><span>${glyph('b')}${esc(t('Back'))}</span></span></div>`;
  }
  const closeShot = () => { const v = $('.tv-viewer', root); if (v) { v.classList.add('out'); setTimeout(() => v.remove(), 200); } TV.shot = null; };
  async function shotAct(name) {
    const list = shotsOf(TV.arg);
    const x = list[TV.shot];
    if (!x) return closeShot();
    if (name === 'left' && TV.shot > 0) { sfx('move'); shotView(TV.shot - 1); }
    else if (name === 'right' && TV.shot < list.length - 1) { sfx('move'); shotView(TV.shot + 1); }
    else if (name === 'b') { sfx('back'); closeShot(); }
    else if (name === 'x') { try { await invoke('copy_screenshot', { path: x.path }); toast(t('Copied. Paste it into Discord, X, a message or anywhere.')); sfx('select'); } catch (e) { fail(e); } }
    else if (name === 'y') {
      try { await invoke('delete_screenshot', { path: x.path }); } catch (e) { return fail(e); }
      S.allShots = S.allShots.filter((s) => s.path !== x.path);
      delete S.shots[x.id];
      const left = shotsOf(TV.arg);
      if (!left.length) { closeShot(); draw(true); } else { draw(true); shotView(Math.min(TV.shot, left.length - 1)); }
    }
  }

  function pageGame(id) {
    const a = byId(id) || (S.library[id] ? { id, name: S.library[id].name, systems: [S.os] } : null);
    if (!a) return pageHome();
    const g = S.library[id];
    const shots = g ? S.shots[id] : null;
    if (g && !shots) loadShots(id).then(() => TV.on && TV.view === 'game' && draw(true));
    if (S.mods[id] === undefined) loadMods(id);
    if (S.info[id] === undefined) loadInfo(id);
    if (g && S.setup[id] === undefined) loadSetup(id);
    const su = S.setup[id] && S.setup[id] !== 'loading' ? S.setup[id] : null;
    const info = S.info[id] && S.info[id] !== 'loading' ? S.info[id] : null;
    const modsOk = !!(S.mods[id] && S.mods[id] !== 'loading');
    const dv = devOf(a);
    const chips = [originName(a.origin), GENRES[a.genre] ? t(GENRES[a.genre]) : ''].filter(Boolean).map(esc)
      .concat((dv && dv.name) || a.author ? [`${dv && dv.avatar ? `<img class="tv-dev-pic" src="${esc(dv.avatar)}" alt="">` : ''}${esc(tf('By {who}', { who: (dv && dv.name) || a.author }))}${dv && dv.verified ? `<em class="tv-vchip">${TICK(20)}${esc(t('Verified developer'))}</em>` : ''}`] : []);
    const checks = readyRows(a);
    const rows = [
      ...checks.map((r) => [r.k, okMark(r.v)]),
      [esc(t('Version')), esc(g ? `${g.tag || t('Not known')} · ${g.pinned ? t('picked by you') : S.updates[id] ? tf('{v} is out', { v: S.updates[id] }) : t('up to date')}` : a.version ? 'v' + String(a.version).replace(/^v/i, '') : '—')],
      g ? [esc(t('Played')), esc(playedFor(g))] : [esc(t('Downloads')), esc(a.downloads ? fmtN(a.downloads) : '—')],
      ...(modsOk && g ? [[esc(t('Mods')), esc(tf('{n} installed', { n: (S.mods[id].installed || []).length }))]] : []),
    ];
    const bad = checks.filter((r) => r.v === false).length;
    const acts = [
      mainButton(a, 'g-main'),
      devOf(a) ? btn('g-follow', 'follow', esc(follows().includes(devOf(a).slug) ? t('Following') : t('Follow')), { id, icon: follows().includes(devOf(a).slug) ? 'check' : 'user' }) : '',
      verGroup(a) ? btn('g-vers', 'versions', esc(tn(verGroup(a).list.length, '1 version', '{n} versions')), { id, g: 'x' }) : '',
      g ? btn('g-shots', 'shots', esc(t('Screenshots')), { id, icon: 'shot' }) : '',
      g && (su || S.os === 'windows') ? btn('g-set', 'gsettings', esc(t('Game settings')) + (su && su.beta && su.settings && su.settings.length ? `<b class="tv-beta-s">${esc(t('Beta'))}</b>` : ''), { id, icon: 'gear' }) : '',
      g && modsOk ? btn('g-mods', 'mods', esc(t('Mods')), { id, icon: 'puzzle' }) : '',
      g ? btn('g-more', 'more', esc(t('More')), { id, icon: 'more' }) : '',
    ].filter(Boolean).join('');
    const about = info ? `<section class="tv-block"><h3>${esc(tf('About {title}', { title: info.title }))}</h3><p class="tv-about">${esc(info.text.split('\n\n')[0])}</p><p class="tv-src">${esc(t('From Wikipedia'))}</p></section>`
      : a.blurb && g ? `<section class="tv-block"><h3>${esc(t('About it'))}</h3><p class="tv-about">${esc(a.blurb)}</p></section>` : '';
    // Not installed yet: what the recomp is and who makes it come first; what it needs comes after.
    const devText = a.blurb || notesLine(a.description);
    const facts = [[t('By'), (devOf(a) && devOf(a).name) || a.author], [t('Version'), a.version ? 'v' + String(a.version).replace(/^v/i, '') : ''], [t('Latest release'), date(a.released)], [t('Downloads'), a.downloads ? fmtN(a.downloads) : ''], [t('Stars on GitHub'), a.stars ? fmtN(a.stars) : ''], [t('Size'), a.size]].filter(([, v]) => v);
    const checkBlock = !g && checks.length ? `<section class="tv-block"><h3>${esc(t('Before you install'))}${bad ? ` <small>${esc(tn(bad, '1 thing to do', '{n} things to do'))}</small>` : ''}</h3><div class="tv-panel tv-checks" data-f data-key="g-checks"><dl>${checks.map((r) => `<div><dt>${esc(r.k)}<small>${esc(r.d || '')}</small></dt><dd>${okMark(r.v)}</dd></div>`).join('')}</dl></div></section>` : '';
    const aside = g
      ? `<aside class="tv-panel" aria-label="${esc(t('Details'))}"><h2>${esc(bad ? t('Before you play') : t('Ready to play'))}</h2><dl>${rows.map(([k, v]) => `<div><dt>${k}</dt><dd>${v}</dd></div>`).join('')}</dl>${tvRuns(a)}</aside>`
      : `<aside class="tv-panel" aria-label="${esc(t('Details'))}"><h2>${esc(t('About this recomp'))}</h2>${devText ? `<p class="tv-devtext">${esc(devText)}</p>` : ''}${tvRuns(a)}<dl>${facts.map(([k, v]) => `<div><dt>${esc(k)}</dt><dd>${esc(v)}</dd></div>`).join('')}</dl></aside>`;
    const shotRow = shots && shots.length ? `<section class="tv-block"><h3>${esc(t('Screenshots'))} <small>${esc(tf('{n} taken', { n: shots.length }))}</small></h3><div class="tv-shots">${shots.slice(0, 4).map((f, i) => `<button type="button" data-f data-key="shot:${i}" data-tvact="shots" data-id="${esc(id)}"><img src="${esc(fileSrc(f))}" alt="${esc(t('Screenshot'))}"></button>`).join('')}</div></section>` : '';
    return `${backdrop(a, 'game')}${gameBar()}
      ${region(`<div class="tv-game">
        <div class="tv-g-main">
          <div class="tv-g-chips">${chips.map((c) => `<span>${c}</span>`).join('')}</div>
          <h1 class="tv-h1">${esc(a.name)}</h1>
          ${a.game ? `<p class="tv-based">${DISC_S.replace(/14/g, '24')}<span>${htmlIn(t('Based on {game}'), { game: `<b>${esc(a.game)}</b>` })}</span>${S.easy.has(id) ? `<span class="tv-easy">${esc(t('Easy Setup'))}</span>` : ''}</p>` : ''}
          ${g && g.last_played ? `<p class="tv-meta">${esc(tf('{time} played · last played {when}', { time: playedFor(g), when: ago(g.last_played) }))}</p>` : ''}
          <div class="tv-acts">${acts}</div>
          ${about}${checkBlock}${shotRow}
        </div>
        ${aside}
      </div>`, 'game')}
      ${hints([['A', t('Select')], ['B', t('Back')], ['LB+RB', t('Sections')]])}`;
  }

  // A game's mods: the ones installed first, then the most popular on Thunderstore or GameBanana.
  // A installs or removes; X switches between the two when the game has both.
  function pageMods(id) {
    const a = byId(id) || { id, name: (S.library[id] || {}).name || id };
    const m = S.mods[id];
    if (m === undefined) loadMods(id);
    const ready = m && m !== 'loading';
    const both = ready && m.community && m.gamebanana;
    const src = ready && m.community && (!m.gamebanana || (S.modSrc[id] || 'ts') === 'ts') ? 'ts' : 'gb';
    const have = new Set(((ready && m.installed) || []).map((x) => x.full));
    const pill = (full) => `<span class="tv-pill${have.has(full) ? ' on' : ''}">${esc(modBusy[full] ? (modBusy[full] === 'in' ? (modProg[full] ? tf('Installing… {pct}%', { pct: modProg[full] }) : t('Installing…')) : t('Removing…')) : have.has(full) ? t('Installed') : t('Install'))}</span>`;
    let mine = [], list = '', head;
    if (src === 'ts') {
      if (!S.modList[id]) loadModList(id);
      const ml = S.modList[id] || { loading: true, list: [] };
      const item = (p, k) => `<button type="button" class="tv-item" data-f data-key="${k}" data-mod="${esc(p.full)}">${p.icon ? `<img src="${esc(media(p.icon))}" alt="" class="sq">` : '<span class="ph sq"></span>'}<span class="k">${esc(p.name)}<small>${esc(p.owner ? tf('By {who} · {n} downloads', { who: p.owner, n: fmtN(p.downloads) }) + ' · ' + p.desc : p.desc)}</small></span>${pill(p.full)}</button>`;
      mine = ((ready && m.installed) || []).filter((x) => !String(x.full).startsWith('gb:')).map((x) => item({ full: x.full, name: x.name, owner: '', desc: tf('Version {v}', { v: x.version }) + (x.dep ? ' · ' + t('another mod needs it') : ''), icon: (ml.list.find((p) => p.full === x.full) || {}).icon }, 'mi:' + x.full));
      list = ml.loading ? `<p class="tv-empty">${esc(t('Loading the mods…'))}</p>` : ml.error ? `<p class="tv-empty">${esc(t(ml.error))}</p>` : (ml.list || []).filter((p) => !have.has(p.full)).slice(0, 30).map((p) => item(p, 'mo:' + p.full)).join('');
      head = t('From Thunderstore. Turn them on and off in the game\'s own Mods menu.');
    } else {
      const g = gbState(id);
      if (ready && !g.mods.length && !g.loading && !g.error && !g.tried) { g.tried = true; gbLoad(id); }
      const item = (x, k) => `<button type="button" class="tv-item" data-f data-key="${k}" data-gbmod="${x.id}">${x.image ? `<img src="${esc(media(x.image))}" alt="" class="wide">` : '<span class="ph wide"></span>'}<span class="k">${esc(x.name)}<small>${esc(x.author ? tf('By {who}', { who: x.author }) + (x.category ? ' · ' + x.category : '') : x.version || '')}</small></span>${pill('gb:' + x.id)}</button>`;
      mine = ((ready && m.installed) || []).filter((x) => String(x.full).startsWith('gb:')).map((x) => item({ id: x.full.slice(3), name: x.name, image: (g.mods.find((y) => 'gb:' + y.id === x.full) || {}).image, version: tf('Version {v}', { v: x.version }) }, 'gi:' + x.full));
      list = g.error ? `<p class="tv-empty">${esc(t(g.error))}</p>` : !g.mods.length ? `<p class="tv-empty">${esc(t('Loading the mods…'))}</p>` : g.mods.filter((x) => !have.has('gb:' + x.id)).slice(0, 30).map((x) => item(x, 'go:' + x.id)).join('');
      head = t('From GameBanana. Mods are turned on when they install.');
    }
    return `${backdrop(a, 'dim')}${gameBar()}
      ${region(`${pageHead(tf('{name}: mods', { name: a.name }), head)}
        ${mine.length ? `<h3 class="tv-subh">${esc(t('Installed'))}</h3><div class="tv-list">${mine.join('')}</div>` : ''}
        <h3 class="tv-subh">${esc(t('Most popular'))}</h3>
        <div class="tv-list">${list}</div>`, 'top')}
      ${hints([['A', t('Install or remove')], ['B', t('Back')]].concat(both ? [['X', src === 'ts' ? t('GameBanana') : t('Thunderstore')]] : []))}`;
  }
  // A GameBanana mod: installed ones come out; otherwise its file goes in (a list when it has a few).
  async function tvGbMod(id, modId) {
    const full = 'gb:' + modId;
    if (modBusy[full] || TV.gbOpening) return;
    const have = ((S.mods[id] || {}).installed || []).some((x) => x.full === full);
    sfx('select');
    if (have) return removeMod(id, full);
    let d;
    TV.gbOpening = true;
    try { d = await invoke('gb_mod', { modId: Number(modId) }); } catch (e) { return fail(e); } finally { TV.gbOpening = false; }
    const files = d.files.filter((f) => f.ok);
    if (!files.length) return toast(t('This mod can\'t be installed from here. Its page on GameBanana says how.'), true);
    const tick = () => { const tm = setInterval(soon, 400); return () => clearInterval(tm); };
    const go = async (f) => { const stop = tick(); await gbInstallFile(id, d, f.id, choose); stop(); soon(); };
    // A mod in versions: which one.
    const choose = (list) => new Promise((resolve) => {
      let picked = null;
      sheet(t('Which version?'), list.map((v) => ({ label: v, run: () => { picked = v; } })));
      TV.sheet.onClose = () => setTimeout(() => resolve(picked), 0);
    });
    if (files.length === 1) return go(files[0]);
    sheet(d.name, files.map((f) => ({ label: f.name, sub: [f.version ? 'v' + f.version : '', fmtB(f.size), f.desc].filter(Boolean).join(' · '), run: () => go(f) })));
  }


  // A game's own settings, set up by the launcher: its copy of the game, then each setting.
  // Left and right (or A) change a setting.
  function pageGameSettings(id) {
    const a = byId(id) || { id, name: (S.library[id] || {}).name || id };
    const su = S.setup[id];
    if (su === undefined) loadSetup(id);
    const win = S.os === 'windows';
    const li = win ? S.looks[id] : null;
    if (win && li === undefined) loadLooks(id);
    const LD = window.Looks && Looks.defs();
    const waiting = su === undefined || su === 'loading' || (win && !(window.Looks && Looks.failed()) && (li === undefined || li === 'loading' || !LD));
    const prof = su && su !== 'loading' ? su : null;
    const look = li && li.ok && LD ? LD.looks.find((l) => l.id === li.look) : null;
    const lookRows = li && li.ok && LD ? [
      `<button type="button" class="tv-item" data-f data-key="gs:__look" data-gset="__look" data-id="${esc(id)}"><span class="k">${esc(t('Look'))}<small>${esc(t('Drawn over the game by ReShade.'))}</small></span><span class="tv-cycle">${svg('chevL', 22)}<b>${esc(look ? t(look.name) : t('Original'))}</b>${svg('chevR', 22)}</span></button>`,
      `<button type="button" class="tv-item${look ? '' : ' off'}" data-f data-key="gs:__amount" data-gset="__amount" data-id="${esc(id)}"${look ? '' : ' disabled'}><span class="k">${esc(t('Amount'))}<small>${esc(t('How strong the look is.'))}</small></span><span class="tv-cycle">${svg('chevL', 22)}<b>${esc(tf('{pct}%', { pct: li.amount }))}</b>${svg('chevR', 22)}</span></button>`,
    ] : [];
    const items = waiting ? `<p class="tv-empty">${esc(t('Loading…'))}</p>` : [
      prof && prof.needs ? `<button type="button" class="tv-item" data-f data-key="gs:copy" data-tvact="setup" data-id="${esc(id)}"><span class="k">${esc(t('Your copy of the game'))}<small>${esc(prof.copy ? baseName(prof.copy) : prof.needs)}</small></span><span class="tv-pill${prof.copy ? ' on' : ''}">${esc(prof.copy ? t('Change') : t('Insert it'))}</span></button>` : '',
      ...lookRows,
      ...(prof ? prof.settings : []).map((o) => `<button type="button" class="tv-item${o.applies ? '' : ' off'}" data-f data-key="gs:${esc(o.id)}" data-gset="${esc(o.id)}" data-id="${esc(id)}"${o.applies ? '' : ' disabled'}><span class="k">${esc(o.label)}<small>${esc(o.help)}</small></span><span class="tv-cycle">${svg('chevL', 22)}<b>${esc(o.choices[o.current] || '')}</b>${svg('chevR', 22)}</span></button>`),
      ...(prof ? prof.actions : []).map((x) => `<button type="button" class="tv-item" data-f data-key="ga:${esc(x.id)}" data-tvact="gaction" data-k="${esc(x.id)}" data-id="${esc(id)}"><span class="k">${esc(x.label)}<small>${esc(x.help)}</small></span><span class="tv-pill on">${esc(x.kind === 'clear' ? t('Clear') : t('Open'))}</span></button>`),
    ].join('') || `<p class="tv-empty">${esc(t('Nothing to set for this game.'))}</p>`;
    const sub = prof && prof.settings.length ? (prof.waiting ? t('Play it once first, then change them here.') : t('Saved in the game\'s own settings, so they\'re the same in its own launcher.')) : t('Looks are drawn over the game. Press Home while you play to fine-tune one.');
    return `${backdrop(a, 'dim')}${gameBar()}
      ${region(`${pageHead(tf('{name}: settings', { name: a.name }), sub)}<div class="tv-setwrap"><div class="tv-list">${items}</div><aside class="tv-setpanel" id="tv-setpanel" aria-live="polite"></aside></div>`, 'top')}
      ${hints([['A', t('Change')], ['B', t('Back')]])}`;
  }
  // Look and Amount: left and right go through the looks, or change the amount by 10.
  async function cycleLook(el, step) {
    const id = el.dataset.id, li = S.looks[id], LD = Looks.defs();
    if (!li || !LD || el.disabled) return;
    sfx('move');
    let look = li.look || '', amount = li.amount;
    if (el.dataset.gset === '__look') {
      const ids = [''].concat(LD.looks.map((l) => l.id));
      look = ids[(ids.indexOf(look) + step + ids.length) % ids.length];
    } else {
      amount = Math.max(0, Math.min(100, amount + step * 10));
    }
    li.look = look; li.amount = amount;
    const b = el.querySelector('.tv-cycle b');
    const l = LD.looks.find((x) => x.id === look);
    if (b) { b.textContent = el.dataset.gset === '__look' ? (l ? t(l.name) : t('Original')) : tf('{pct}%', { pct: amount }); b.classList.remove('bump'); void b.offsetWidth; b.classList.add('bump'); }
    const amt = $('[data-gset="__amount"]', root);
    if (amt) { amt.disabled = !l; amt.classList.toggle('off', !l); }
    setPanel(el);
    // Saved a moment after the last press, one save per game.
    const prev = lookSaves[id] ? lookSaves[id].prev : { look: (S.library[id] || {}).look || '', amount: (S.library[id] || {}).look_amount ?? 50 };
    if (lookSaves[id]) clearTimeout(lookSaves[id].t);
    lookSaves[id] = { prev, t: setTimeout(() => saveLook(id), 250) };
  }
  const lookSaves = {};
  async function saveLook(id) {
    const job = lookSaves[id];
    if (!job) return;
    clearTimeout(job.t);
    delete lookSaves[id];
    const li = S.looks[id];
    if (!li || li === 'loading') return;
    const want = { look: li.look || '', amount: li.amount };
    try {
      await invoke('set_look', { id, look: want.look, amount: want.amount });
      if (S.library[id]) { S.library[id].look = want.look; S.library[id].look_amount = want.amount; }
    } catch (e) {
      // Not saved: show what the game will really start with.
      li.look = job.prev.look; li.amount = job.prev.amount;
      fail(e);
      soon();
    }
  }
  window.tvFlushLooks = () => Promise.all(Object.keys(lookSaves).map(saveLook));
  async function cycleSetting(el, step) {
    const id = el.dataset.id, k = el.dataset.gset, su = S.setup[id];
    if (k === '__look' || k === '__amount') return cycleLook(el, step);
    const o = su && su.settings && su.settings.find((x) => x.id === k);
    if (!o || !o.applies) return;
    sfx('move');
    const i = (o.current + step + o.choices.length) % o.choices.length;
    o.current = i;
    const b = el.querySelector('.tv-cycle b');
    if (b) { b.textContent = o.choices[i]; b.classList.remove('bump'); void b.offsetWidth; b.classList.add('bump'); }
    setPanel(el);
    try { await invoke('set_game_setting', { id, setting: k, choice: i }); } catch (e) { fail(e); }
    delete S.setup[id];
    await loadSetup(id);
  }

  // A short list over the page (More on a game page): A picks, B closes.
  function sheet(title, items, cls) {
    TV.sheet = { items, prev: (focused() || {}).dataset?.key };
    const el = document.createElement('div');
    el.className = 'tv-sheet' + (cls ? ' ' + cls : '');
    el.innerHTML = `<div class="tv-sheet-in"><h3>${esc(title)}</h3>${items.map((x, i) => `<button type="button" class="tv-item" data-f data-key="sheet:${i}" data-sheet="${i}">${x.icon ? svg(x.icon) : ''}<span class="k">${esc(x.label)}${x.sub ? `<small>${esc(x.sub)}</small>` : ''}</span></button>`).join('')}</div>`;
    stage().append(el);
    focus($('[data-sheet="0"]', el), true);
    sfx('open');
  }
  // How it runs, in one line and a bar: what players said about it.
  function tvRuns(a) {
    const r = runTally((S.runCounts || {})[a.id]);
    if (!r.n) return `<div class="tv-runs none"><span class="k">${esc(t('How it runs'))}</span><b>${esc(t('No reports yet'))}</b></div>`;
    return `<div class="tv-runs"><span class="k">${esc(t('How it runs'))}</span><div class="tv-runs-l"><b>${esc(tf('{pct}% plays great', { pct: r.pct('runs') }))}</b><small>${esc(tn(r.n, '1 report', '{n} reports'))}</small></div>${runBar(r)}</div>`;
  }
  // The versions of a game: pick one and its page takes this one's place.
  function pickVersion(id) {
    const a = byId(id), g = verGroup(a);
    if (!g) return;
    const items = g.list.map((v) => ({
      label: v.name,
      sub: [v.id === g.pick ? t('Recommended') : '', S.library[v.id] ? t('Installed') : '', v.author, v.adds, v.downloads ? tf('{n} downloads', { n: fmtN(v.downloads) }) : ''].filter(Boolean).join(' · '),
      icon: v.id === id ? 'check' : 'layers',
      run: () => {
        if (v.id === TV.arg) return;
        TV.arg = v.id;
        TV.enter = 'in';
        TV.scrollY = 0;
        TV.focusKey = 'g-vers';
        sfx('select');
        draw();
        const vb = $('[data-key="g-vers"]', root);
        if (vb) focus(vb, true);
      },
    }));
    sheet(tf('{n} versions of {game}', { n: g.list.length, game: g.name }), items, 'tv-versheet');
    TV.sheet.vers = true;
    const at = g.list.findIndex((v) => v.id === id);
    const el = $(`.tv-sheet:not(.out) [data-sheet="${Math.max(0, at)}"]`, root);
    if (el) focus(el, true);
  }
  // A question as a sheet, for app.js (cloud saves before Play): resolves to the pick, or -1.
  window.tvAsk = (title, items) => new Promise((res) => {
    if (!TV.on) return res(null);
    let done = false;
    const fin = (v) => { if (!done) { done = true; res(v); } };
    sheet(title, items.map((x, i) => ({ ...x, run: () => fin(i) })));
    TV.sheet.onClose = () => setTimeout(() => fin(-1), 0);
  });
  // Signing in from the couch: a QR code for a phone, and the code to type.
  window.tvSignIn = (s) => {
    if (!TV.on) return null;
    TV.sheet = { items: [{ label: t('Cancel') }], prev: (focused() || {}).dataset?.key, onClose: () => s.onClose && s.onClose() };
    const el = document.createElement('div');
    el.className = 'tv-sheet tv-signin';
    el.innerHTML = `<div class="tv-sheet-in"><h3>${esc(t('Sign in with your phone'))}</h3><div class="tv-signin-two"><div class="tv-qr">${s.qr}</div><div><p>${esc(t('Scan the code with your phone, or go to recomplauncher.com/activate and type:'))}</p><div class="tv-code">${esc(s.code)}</div><p class="tv-wait" data-st>${esc(t('Waiting for you to sign in…'))}</p></div></div>
      <button type="button" class="tv-item" data-f data-key="sheet:0" data-sheet="0"><span class="k">${esc(t('Cancel'))}</span></button></div>`;
    stage().append(el);
    focus($('[data-sheet="0"]', el), true);
    sfx('open');
    return {
      say: (m) => { const p = $('[data-st]', el); if (p) p.textContent = m; },
      close: (quiet) => { if (quiet && TV.sheet) TV.sheet.onClose = null; if (topSheet() === el) closeSheet(); },
    };
  };
  // The sheet on top (the file browser can be over the setup sheet).
  const topSheet = () => $$('.tv-sheet:not(.out)', root).pop();
  function closeSheet() {
    if (TV.sheet && TV.sheet.browse) return tbClose(null);
    if (TV.sheet && TV.sheet.onClose) TV.sheet.onClose();
    const el = topSheet();
    if (el) { el.classList.add('out'); setTimeout(() => el.remove(), 200); }
    const prev = TV.sheet && TV.sheet.prev;
    TV.sheet = null;
    const back = prev && $(`.tv-stage > :not(.tv-sheet) [data-key="${CSS.escape(prev)}"]`, root);
    if (back) focus(back, true);
  }

  // ---------- picking a file or folder with a controller ----------
  // The launcher's own browser, full screen: places on the left, the folder on the right. A opens or
  // picks, B goes up a folder (and leaves at the top), X uses the open folder, Y jumps to places.
  const TB = { el: null };
  window.tvBrowse = async (opts) => {
    let places;
    try { places = await invoke('fs_places'); } catch (e) { return null; }
    if (TB.el) tbClose(null);
    return new Promise((resolve) => {
      const el = document.createElement('div');
      el.className = 'tv-sheet tv-browse';
      stage().append(el);
      Object.assign(TB, { el, opts, places, path: '', list: null, games: {}, resolve, prevSheet: TV.sheet, prevKey: (focused() || {}).dataset?.key, n: 0 });
      sfx('open');
      tbGo(fbStart(opts, places));
    });
  };
  function tbClose(v) {
    if (!TB.el) return;
    const el = TB.el, resolve = TB.resolve;
    TB.el = null;
    el.classList.add('out');
    setTimeout(() => el.remove(), 200);
    TV.sheet = TB.prevSheet || null;
    const back = TB.prevKey && $(`.tv-stage [data-key="${CSS.escape(TB.prevKey)}"]:not(.tv-browse *)`, root);
    if (back) focus(back, true);
    resolve(v);
  }
  async function tbGo(path, keepKey) {
    if (!TB.el || !path) return;
    const n = ++TB.n;
    let v;
    try { v = await invoke('fs_list', { path }); } catch (e) { v = { path, parent: null, entries: [], denied: true, games: {} }; }
    if (!TB.el || n !== TB.n) return;
    const from = TB.path;
    Object.assign(TB, { list: v, games: v.games || {}, path: v.path });
    tbDraw(keepKey || (from && from.startsWith(v.path) && from !== v.path ? 'tb:e:' + from : null));
  }
  function tbPick(p) {
    if (!p) return;
    fbRemember(TB.opts, TB.opts.directory ? p : TB.path);
    sfx('select');
    tbClose(p);
  }
  function tbDraw(focusKey) {
    const el = TB.el, o = TB.opts, P = TB.places;
    const items = [];
    const item = (key, html, run, cls = '') => { items.push({ run }); return `<button type="button" class="tv-item ${cls}" data-f data-key="${esc(key)}" data-sheet="${items.length - 1}">${html}</button>`; };
    const place = (p) => item('tb:p:' + p.path, `${svg(p.kind === 'drive' ? 'disk' : p.kind === 'games' ? 'disc' : 'folder', 24)}<span class="k">${esc(p.kind === 'drive' ? tf('Drive {name}', { name: p.name }) : ['desktop', 'documents', 'downloads', 'home'].includes(p.kind) ? t(p.name) : p.name)}${p.kind === 'drive' && p.total ? `<small>${esc(tf('{free} free', { free: fmtB(p.free) }))}</small>` : ''}</span>`, () => tbGo(p.path), TB.path === p.path ? 'on' : '');
    const sec = (title, list) => (list.length ? `<h4>${esc(title)}</h4>${list.map(place).join('')}` : '');
    const pins = fbPins().map((p) => ({ path: p, name: p.replace(/[\\/]+$/, '').split(/[\\/]/).pop() || p, kind: 'folder' }));
    const side = sec(t('Pinned'), pins) + sec(t('Your game folders'), P.games) + sec(t('Quick access'), P.quick) + sec(t('Drives'), P.drives) + sec(t('Game library'), P.library);
    const L = TB.list || { entries: [] };
    const rows = L.entries.filter((e) => fbTakes(o, e));
    const list = L.denied ? `<p class="tv-empty">${esc(t('This folder can\'t be opened.'))}</p>`
      : !rows.length ? `<p class="tv-empty">${esc(o.directory ? t('No folders in here. Use this one, or go up.') : t('Nothing you can pick in here.'))}</p>`
        : rows.slice(0, 400).map((e) => {
          const g = TB.games[e.path];
          const icon = e.dir ? (e.kind === 'game' ? 'disc' : 'folder') : g || e.kind === 'disc' ? 'disc' : 'file';
          return item('tb:e:' + e.path, `${svg(icon, 24)}<span class="k">${esc(e.name)}${g ? `<small class="tb-game">${esc(g.name)}</small>` : !e.dir && e.size ? `<small>${esc(fmtB(e.size))}</small>` : ''}</span>${e.dir ? svg('chevR', 22) : ''}`, () => (e.dir ? tbGo(e.path) : tbPick(e.path)), g ? 'has' : '');
        }).join('');
    const refresh = item('tb:refresh', `${svg('refresh', 24)}<span class="k">${esc(t('Refresh'))}</span>`, () => tbGo(TB.path, 'tb:refresh'));
    const useHere = o.directory ? item('tb:use', `<span class="k">${esc(t('Use this folder'))}<small>${esc(TB.path)}</small></span>`, () => tbPick(TB.path), 'main') : '';
    el.innerHTML = `<div class="tb-in"><aside class="tb-side">${side}</aside><section class="tb-main"><div class="tb-head"><h2>${esc(o.title || t('Choose'))}</h2><p>${esc(TB.path)}</p></div>${useHere}${refresh}<div class="tb-list">${list}</div></section></div>
      ${hints([['A', t('Open')], ['B', L.parent ? t('Up a folder') : t('Back')]].concat(o.directory ? [['X', t('Use this folder')]] : [], [['Y', t('Places')]]))}`;
    TV.sheet = {
      items, keep: true, browse: true, prev: TB.prevKey,
      back: () => { if (TB.list && TB.list.parent) tbGo(TB.list.parent); else tbClose(null); return true; },
      keys: (name, cur) => {
        if (name === 'x' && o.directory) { tbPick(TB.path); return true; }
        if (name === 'y') { const p = $('.tb-side .tv-item.on', el) || $('.tb-side .tv-item', el); if (p) { focus(p); sfx('move'); } return true; }
        // The bumpers and Menu would switch pages under it: they do nothing here.
        return ['lb', 'rb', 'menu', 'x'].includes(name);
      },
    };
    const f = (focusKey && $(`[data-key="${CSS.escape(focusKey)}"]`, el)) || $('.tb-list .tv-item', el) || $('.tb-main .tv-item', el) || $('.tb-side .tv-item', el);
    if (f) focus(f, true);
  }

  // ---------- setting a game up with the player's copy, full screen ----------
  const TS = { id: null, stage: 'pick', step: '', steps: {}, q: [], at: 0, path: '', extras: false, result: null, err: '' };
  async function tvSetup(id) {
    if (TS.id && TS.stage === 'run' && TS.id !== id) return toast(t('Finish the setup that\'s running first.'));
    if (S.setup[id] === undefined || S.setup[id] === 'loading') { try { S.setup[id] = await invoke('game_setup', { id }); } catch (e) { S.setup[id] = null; } }
    const su = S.setup[id];
    if (!su || !su.needs) return toast(t('This game asks for your copy itself the first time it starts.'));
    if (TV.sheet) closeSheet();
    Object.assign(TS, { id, stage: 'pick', step: '', steps: {}, q: [], at: 0, path: '', extras: false, result: null, err: '' });
    tsDraw();
    sfx('open');
  }
  function tsDraw() {
    const st = stage();
    if (!st || !TS.id) return;
    const id = TS.id, a = byId(id) || { name: (S.library[id] || {}).name || id }, su = S.setup[id] || {};
    let el = $('.tv-setup:not(.out)', st);
    const prevKey = (focused() || {}).dataset?.key;
    if (!el) { el = document.createElement('div'); el.className = 'tv-sheet tv-setup'; st.append(el); }
    const cov = a.cover ? media(a.cover) : '';
    const label = `<div class="lab">${cov ? `<img src="${esc(cov)}" alt="">` : ''}</div><div class="sheen"></div><div class="hub"></div><div class="hole"></div>`;
    let items = [], body = '';
    const it = (label, sub, run, cls = '') => { items.push({ run }); const i = items.length - 1; return `<button type="button" class="tv-item ${cls}" data-f data-key="ts:${i}" data-sheet="${i}"><span class="k">${esc(label)}${sub ? `<small>${esc(sub)}</small>` : ''}</span></button>`; };
    const pickIt = async (folder) => { const p = await pickFile(folder ? { title: t('The game\'s folder'), directory: true } : { title: t('Your disc image of the game'), directory: false, multiple: false, filters: [{ name: t('Disc images'), extensions: ['iso', 'xiso', 'img', 'bin'] }, { name: t('All files'), extensions: ['*'] }] }).catch(fail); if (p) tsBegin(typeof p === 'string' ? p : p.path); else tsDraw(); };
    if (TS.stage === 'pick') {
      const own = S.owned[id];
      body = `<div class="bigdisc ghost"></div><div class="tt"><h2>${esc(tf('Insert your copy of {game}', { game: a.game || a.name }))}</h2><p>${esc(su.needs || '')}</p></div>
        <div class="opts">${own ? it(t('Use the one in your folders'), own.path, () => tsBegin(own.path), 'main') : ''}${su.kind !== 'folder' ? it(t('Choose a disc image'), t('An .iso of your disc'), () => pickIt(false)) : ''}${su.kind === 'iso_or_folder' || su.kind === 'folder' ? it(t('Choose a game folder'), t('A folder you copied from the disc'), () => pickIt(true)) : ''}</div>`;
    } else if (TS.stage === 'run') {
      const order = ['read', 'verify'].concat(TS.extras ? ['extras'] : [], ['setup']);
      const names = { read: N_('Reading your disc'), verify: N_('Checking your copy'), extras: N_('Getting the extras'), setup: N_('Setting it up') };
      const s = TS.steps, cur = order.indexOf(TS.step);
      const sub = { read: (s.read && s.read.text) || baseName(TS.path), verify: s.verify && s.verify.text ? s.verify.text : s.verify && s.verify.total ? tf('{done} of {total}', { done: fmtB(s.verify.done), total: fmtB(s.verify.total) }) : t('Reading all of it'), extras: (s.extras && s.extras.text) || t('What the game needs from your disc'), setup: t('Settings, controller and your copy') };
      const fill = (k, i) => (i < cur ? 100 : i === cur ? Math.round(((s[k] && s[k].pct) || 0) * 100) : 0);
      const weight = { read: 5, verify: TS.extras ? 70 : 80, extras: 15, setup: 10 };
      let tot = 0, have = 0;
      order.forEach((k, i) => { tot += weight[k]; have += weight[k] * fill(k, i) / 100; });
      if (el.dataset.stage === 'run' && el.dataset.step === TS.step + order.length && $('.track', el)) {
        $('.pct', el).textContent = tf('{pct}%', { pct: Math.floor((have / tot) * 100) });
        $$('.tk', el).forEach((tk, i) => { $('u', tk).style.width = fill(order[i], i) + '%'; $('span', tk).textContent = sub[order[i]]; });
        return;
      }
      el.dataset.step = TS.step + order.length;
      body = `<div class="bigdisc ${cur <= 0 ? 'in' : 'spin'}">${label}</div><div class="tt"><div class="pct">${esc(tf('{pct}%', { pct: Math.floor((have / tot) * 100) }))}</div><p>${esc(tf('Setting up {name}', { name: a.name }))}</p></div>
        <div class="track" style="grid-template-columns:repeat(${order.length},minmax(0,1fr))">${order.map((k, i) => `<div class="tk${i > cur ? ' w' : ''}"><i><u style="width:${fill(k, i)}%"></u></i><b>${esc(t(names[k]))}</b><span>${esc(sub[k])}</span></div>`).join('')}</div>`;
    } else if (TS.stage === 'ready') {
      const r = TS.result || {};
      items = [{ run: () => playGame(id) }, { run: () => { if (TV.view === 'gsettings' && TV.arg === id) draw(true); else go('gsettings', id); } }];
      body = `<div class="bigdisc">${label}</div><div class="tt"><span class="ok${r.verified ? '' : ' plain'}">${esc(r.text || t('Checked'))}</span><h2>${esc(t('Ready to play'))}</h2><p>${esc(tf('{name} is set up with your copy.', { name: a.name }))}</p></div>
        <div class="two"><button type="button" class="tv-btn main" data-f data-key="ts:0" data-sheet="0">${glyph('a', 'tv-keyg')}<span>${esc(t('Play'))}</span></button><button type="button" class="tv-btn" data-f data-key="ts:1" data-sheet="1"><span>${esc(t('Settings'))}</span></button></div>`;
    } else {
      items = [{ run: () => { TS.stage = 'pick'; setTimeout(tsDraw, 0); } }];
      body = `<div class="bigdisc ghost"></div><div class="tt"><h2>${esc(t('That copy didn\'t work'))}</h2><p>${esc(t(TS.err))}</p></div><div class="two" style="grid-template-columns:1fr;width:520px"><button type="button" class="tv-btn main" data-f data-key="ts:0" data-sheet="0">${glyph('a', 'tv-keyg')}<span>${esc(t('Try another copy'))}</span></button></div>`;
    }
    el.dataset.stage = TS.stage;
    el.innerHTML = `${cov ? `<div class="glow" style="background-image:url('${cssUrl(cov)}')"></div>` : ''}${body}`;
    // B leaves: it stops a setup that's running.
    TV.sheet = { items, keep: TS.stage !== 'ready', prev: TV.sheet ? TV.sheet.prev : prevKey, onClose: () => { if (TS.stage === 'run') invoke('setup_cancel', { id }).catch(() => {}); if (TS.stage === 'ready') { delete S.setup[id]; loadSetup(id); } TS.id = null; } };
    const f = $('[data-sheet]', el);
    if (f) focus(f, true);
  }
  async function tsBegin(path) {
    Object.assign(TS, { stage: 'run', path, step: 'read', steps: {}, q: [], at: Date.now(), extras: false });
    tsDraw();
    try { await invoke('setup_copy', { id: TS.id, path }); } catch (e) { TS.stage = 'error'; TS.err = String(e && e.message ? e.message : e); tsDraw(); }
  }
  const TS_ORDER = ['read', 'verify', 'extras', 'setup', 'done'];
  function tsPump() {
    clearTimeout(TS.timer);
    while (TS.q.length) {
      const ev = TS.q[0];
      if (ev.step === 'error') { TS.q = []; TS.stage = 'error'; TS.err = ev.text; sfx('back'); return tsDraw(); }
      if (TS_ORDER.indexOf(ev.step) > TS_ORDER.indexOf(TS.step)) {
        const wait = 750 - (Date.now() - TS.at);
        if (wait > 0) { TS.timer = setTimeout(tsPump, wait); return; }
        TS.step = ev.step; TS.at = Date.now();
      }
      TS.q.shift();
      TS.extras = ev.extras;
      TS.steps[ev.step] = Object.assign({}, TS.steps[ev.step], ev);
      if (ev.step === 'done') { TS.stage = 'ready'; TS.result = ev; sfx('select'); return tsDraw(); }
    }
    tsDraw();
  }
  T.event.listen('setup', ({ payload: p }) => { if (TV.on && TS.id && p.id === TS.id && TS.stage === 'run') { TS.q.push(p); tsPump(); } });
  window.tvSetup = tvSetup;
  window.tvSetupBusy = () => !!(TV.on && TS.id);

  async function versionsSheet(id) {
    let list = [];
    try { list = await invoke('versions', { id }); } catch (e) { return fail(e); }
    const g = S.library[id];
    const items = list.slice(0, 8).map((v) => ({ label: v.name || v.tag, sub: [v.tag, v.newest ? t('the newest') : '', g && g.tag === v.tag ? t('installed') : '', date(v.date)].filter(Boolean).join(' · '), run: () => (g && g.tag === v.tag ? null : startInstall(id, undefined, v.newest ? null : v.tag)) }));
    if (!items.length) return toast(t('No other versions for this PC.'));
    sheet(t('Other versions'), items);
  }

  // ---------- drawing and focus ----------
  const VIEWS = { home: pageHome, library: pageLibrary, discover: pageDiscover, downloads: pageDownloads, settings: pageSettings };
  function draw(keep) {
    if (!TV.on) return;
    const prevKey = keep ? (focused() || {}).dataset?.key : null;
    const prevScroll = keep ? TV.scrollY || 0 : 0;
    const v = TV.view;
    const html = v === 'game' ? pageGame(TV.arg) : v === 'shots' ? pageShots(TV.arg) : v === 'mods' ? pageMods(TV.arg) : v === 'gsettings' ? pageGameSettings(TV.arg) : (VIEWS[v] || pageHome)();
    const anim = keep ? '' : TV.enter || 'in';
    if (!keep) TV.panelKey = null;
    TV.enter = null;
    const fresh = document.createElement('div');
    fresh.className = `tv-stage${anim ? ' tvx-' + anim : ''}`;
    fresh.innerHTML = html;
    const old = stage();
    // A screenshot or a sheet that's open stays open across a redraw.
    const keepers = keep && old ? $$(':scope > .tv-viewer, :scope > .tv-sheet', old) : [];
    if (old) old.replaceWith(fresh); else root.prepend(fresh);
    keepers.forEach((k) => { k.style.animation = 'none'; fresh.append(k); });
    fit();
    fitAll();
    TV.swap = false;
    if (keep) setScroll(prevScroll, true);
    movePill();
    const pick = (k) => k && $(`.tv-stage > :not(.tv-sheet) [data-key="${CSS.escape(k)}"]`, root);
    // Where focus goes: back where it was, else the page's main button, its first item, or the
    // section's tab. If the page was still loading when it got the tab, its first item once it's there.
    const first = $('.tv-region .tv-btn.main[data-f]:not([disabled])', root) || $('.tv-region [data-f]:not([disabled])', root);
    let want = (TV.sheet && topSheet() && ($('[data-f].f', topSheet()) || $('[data-f]', topSheet()))) || (TV.parked && first) || pick(prevKey) || pick(TV.focusKey) || first;
    TV.parked = false;
    if (!want) { want = $('.tv-tab.on', root) || $('.tv-back', root) || $('[data-f]', root); TV.parked = true; }
    TV.focusKey = null;
    if (want) focus(want, true);
  }
  window.tvRefresh = () => soon();
  window.tvGoto = (view, id) => {
    if (!TV.on) return;
    if (view === 'shots') { loadAllShots().then(() => go('shots', id || null)); }
    else go(view, id || null);
  };

  const focused = () => $('.tv-stage [data-f].f', root);

  function focus(el, instant) {
    if (!el) return;
    const old = focused();
    if (old && old !== el) old.classList.remove('f');
    el.classList.add('f');
    // Home: the backdrop and the hero follow the focused game.
    const hid = el.dataset.hero;
    if (hid && TV.view === 'home' && hid !== TV.hero && !el.closest('.tv-hero')) {
      TV.hero = hid;
      const a = byId(hid) || { id: hid, name: (S.library[hid] || {}).name || hid };
      const bg = $('#tv-bg', root), hero = $('#tv-hero', root);
      if (bg) {
        const img = a.hero || a.cover ? `<img src="${esc(heroOf(a))}" alt="" class="in">` : '<div class="tv-glow"></div>';
        for (const o of $$('img', bg)) { o.classList.add('gone'); setTimeout(() => o.remove(), 500); }
        bg.insertAdjacentHTML('beforeend', img);
      }
      if (hero) { hero.innerHTML = heroText(a, S.library[hid]); hero.classList.remove('swap'); void hero.offsetWidth; hero.classList.add('swap'); fitAll(); }
    }
    if (TV.view === 'gsettings' || TV.view === 'settings') setPanel(el);
    keepInView(el, instant);
  }
  // The focused setting, large: its value with arrows, every choice as a dot, and what it does.
  function setPanel(el) {
    const p = $('#tv-setpanel', root);
    if (!p || !el) return;
    const su = S.setup[TV.arg] || {}, k = el.dataset.gset;
    let html, preview = false;
    if (k === '__look' || k === '__amount') {
      const li = S.looks[TV.arg] || {}, LD = Looks.defs();
      const ids = [''].concat(LD.looks.map((l) => l.id));
      const l = LD.looks.find((x) => x.id === li.look);
      const isLook = k === '__look';
      html = `<span class="k">${esc(isLook ? t('Look') : t('Amount'))}</span><div class="tv-lkprev"><canvas></canvas></div>
        <div class="val">${svg('chevL', 34)}<b>${esc(isLook ? (l ? t(l.name) : t('Original')) : tf('{pct}%', { pct: li.amount }))}</b>${svg('chevR', 34)}</div>
        <div class="dots">${isLook ? ids.map((x) => `<i class="${x === (li.look || '') ? 'on' : ''}"></i>`).join('') : [0, 20, 40, 60, 80, 100].map((x) => `<i class="${li.amount >= x && (x === 100 || li.amount < x + 20) ? 'on' : ''}"></i>`).join('')}</div>
        <p>${esc(isLook ? (l ? t(l.blurb) : t('The game as it comes, with nothing on top.')) : t('Light to strong.'))}</p>`;
      preview = true;
    } else if (k) {
      const o = (su.settings || []).find((x) => x.id === k);
      if (!o) return;
      html = `<span class="k">${esc(o.applies ? t('Setting') : t('Not used right now'))}</span><h3>${esc(o.label)}</h3>
        <div class="val">${svg('chevL', 34)}<b>${esc(o.choices[o.current] || '')}</b>${svg('chevR', 34)}</div>
        <div class="dots">${o.choices.map((c, i) => `<i class="${i === o.current ? 'on' : ''}" title="${esc(c)}"></i>`).join('')}</div><p>${esc(o.help)}</p>`;
    } else if (el.dataset.key === 'gs:copy') {
      html = `<span class="k">${esc(t('Your copy'))}</span><h3>${esc(su.copy ? t('Set up') : t('Not set up yet'))}</h3><p>${esc(su.copy || su.needs || '')}</p>`;
    } else if (el.dataset.key === 's:buttons' && window.buttonGuide) {
      html = `<span class="k">${esc(t('Controller'))}</span><div class="tv-padguide">${window.buttonGuide(glyph).map(([k, n, d]) => `<div><span class="pg-k">${k}</span><span class="pg-t"><b>${esc(n)}</b></span></div>`).join('')}</div>`;
    } else if (TV.view === 'settings') {
      // Launcher settings: what it is, its value, and what it does.
      const kEl = $('.k', el), on = el.dataset.sw;
      const val = on != null ? (on === '1' ? t('On') : t('Off')) : el.dataset.val || '';
      html = `<span class="k">${esc(t('Setting'))}</span><h3>${esc(kEl ? kEl.firstChild.textContent : '')}</h3>${val ? `<div class="val solo"><b>${esc(val)}</b></div>` : ''}<p>${esc(($('small', el) || {}).textContent || '')}</p>`;
    } else {
      const kEl = $('.k', el);
      html = `<span class="k">${esc(t('Button'))}</span><h3>${esc(kEl ? kEl.firstChild.textContent : '')}</h3><p>${esc(($('small', el) || {}).textContent || '')}</p>`;
    }
    // Fades only when it moves to another setting, not when the page redraws around the same one
    // (changing a setting redraws it).
    const key = el.dataset.key || '';
    if (p.dataset.k === key && !k) return;
    const swap = TV.panelKey !== key;
    TV.panelKey = key;
    p.dataset.k = key;
    // The preview stays put while the look changes, so it doesn't flash.
    const old = preview && !swap ? $('.tv-lkprev canvas', p) : null;
    p.innerHTML = html;
    if (old) $('.tv-lkprev', p).replaceChildren(old);
    if (swap) { p.classList.remove('swap'); void p.offsetWidth; p.classList.add('swap'); }
    if (preview) {
      const id = TV.arg, li = S.looks[id] || {};
      lookPicture(id).then((img) => {
        const c = $('.tv-lkprev canvas', p);
        if (!img || !c) return;
        const w = Math.round(c.clientWidth * (window.devicePixelRatio || 1) * (TV.k || 1));
        if (c.width !== w) { c.width = w; c.height = Math.round(w * 9 / 16); }
        Looks.draw(c, img, li.look || '', li.amount, li.lines);
      });
    }
  }

  // The region scrolls up and down; rows slide sideways.
  function setScroll(y, instant) {
    const sc = $('#tv-scroll', root);
    if (!sc) return;
    const reg = sc.parentElement;
    const max = Math.max(0, sc.scrollHeight - reg.clientHeight + 40);
    y = Math.max(0, Math.min(max, y));
    TV.scrollY = y;
    if (instant) sc.style.transition = 'none';
    sc.style.transform = `translateY(${-y}px)`;
    // The settings panel beside a long list stays in view as the list moves.
    const sp = $('#tv-setpanel', sc);
    if (sp) {
      const wrap = sp.parentElement;
      let wt = 0, n2 = wrap;
      while (n2 && n2 !== sc) { wt += n2.offsetTop; n2 = n2.offsetParent; }
      const room = Math.max(0, wrap.offsetHeight - sp.offsetHeight);
      if (instant) sp.style.transition = 'none';
      sp.style.transform = `translateY(${Math.round(Math.min(room, Math.max(0, y - wt + 24)))}px)`;
      if (instant) requestAnimationFrame(() => { sp.style.transition = ''; });
    }
    if (instant) requestAnimationFrame(() => { sc.style.transition = ''; });
    const hero = $('#tv-hero', root);
    if (hero) hero.style.opacity = y > 60 ? '0' : '1';
  }
  function keepInView(el, instant) {
    const r = el.closest('.tv-row');
    if (r) {
      const view = r.parentElement, items = $$('[data-f]', r);
      const i = items.indexOf(el);
      // Keep one tile before the focused one in view (two near the end), and never slide past the last.
      const lead = items[Math.max(0, i - 1)];
      const maxX = Math.max(0, r.scrollWidth - (view.clientWidth - SX * 2));
      // A tile can sit in a wrapper (for its badges), so measure the row's own child.
      const cell = lead && ([...r.children].find((c) => c.contains(lead)) || lead);
      const x = i === 0 || !cell ? 0 : Math.max(0, Math.min(cell.offsetLeft, maxX));
      if (instant) r.style.transition = 'none';
      r.style.transform = `translateX(${-x}px)`;
      if (instant) requestAnimationFrame(() => { r.style.transition = ''; });
    }
    // The file browser's lists and a long sheet scroll on their own.
    const box = el.closest('.tb-list, .tb-side, .tv-sheet-in');
    if (box) {
      const top = el.offsetTop - box.offsetTop, pad = 24;
      if (top - box.scrollTop < pad) box.scrollTo({ top: Math.max(0, top - pad), behavior: instant ? 'auto' : 'smooth' });
      else if (top + el.offsetHeight - box.scrollTop > box.clientHeight - pad) box.scrollTo({ top: top + el.offsetHeight - box.clientHeight + pad, behavior: instant ? 'auto' : 'smooth' });
      return;
    }
    if (el.closest('.tv-bar, .tv-sheet, .tv-viewer')) return;
    const sc = $('#tv-scroll', root);
    if (!sc || !sc.contains(el)) return;
    const reg = sc.parentElement, H = reg.clientHeight;
    // Where the element sits in the scrolling content.
    let top = 0, n = el;
    while (n && n !== sc) { top += n.offsetTop; n = n.offsetParent; }
    const h = el.offsetHeight;
    let y = TV.scrollY || 0;
    if (TV.view === 'home') {
      // Home: the hero, then each row moving up into its place.
      const sec = el.closest('.tv-sec');
      y = !sec ? 0 : sec.offsetTop - 24;
      if (sec === $('.tv-sec', sc)) y = Math.max(0, sec.offsetTop - (H - sec.offsetHeight) + 20);
    } else {
      const pad = 70;
      if (top - y < pad) y = top - pad;
      if (top + h - y > H - pad) y = top + h - H + pad;
      if (top < 260) y = 0; // near the top: the page's heading stays in view
    }
    setScroll(y, instant);
  }

  // The genre chips' highlight slides from one to the next.
  function movePill() {
    const p = $('.tv-chip-pill', root), on = $('.tv-chip.on', root);
    if (!p || !on) return;
    const from = TV.pill;
    const to = { x: on.offsetLeft, w: on.offsetWidth };
    p.style.transition = 'none';
    if (from) { p.style.transform = `translateX(${from.x}px)`; p.style.width = from.w + 'px'; }
    void p.offsetWidth;
    p.style.transition = '';
    p.style.transform = `translateX(${to.x}px)`;
    p.style.width = to.w + 'px';
    TV.pill = to;
  }

  // Up, down, left and right: the nearest thing in that direction, rows first.
  function move(dir) {
    TV.parked = false;
    const cur = focused();
    const scope = (TV.sheet && topSheet()) || stage();
    const all = $$('[data-f]', scope).filter((e) => (!e.disabled || e === cur) && !e.closest('.tv-sheet.out'));
    if (!cur) { if (all[0]) focus(all[0]); return; }
    // Within a row, left and right go to the neighbour wherever it has slid to.
    const rowEl = cur.closest('.tv-row, .tv-acts, .tv-shots, .tv-nav');
    if (rowEl && (dir === 'left' || dir === 'right')) {
      const items = $$('[data-f]', rowEl).filter((e) => !e.disabled);
      const n = items[items.indexOf(cur) + (dir === 'right' ? 1 : -1)];
      if (n) return focus(n);
      if (rowEl.classList.contains('tv-row') || rowEl.classList.contains('tv-nav')) return;
    }
    // Rows remember where you were; coming back lands there.
    const fromRow = cur.closest('.tv-row');
    if (fromRow) TV.row[fromRow.dataset.row] = cur.dataset.key;
    const r = cur.getBoundingClientRect();
    const cx = r.left + r.width / 2, cy = r.top + r.height / 2;
    const regionEl = $('.tv-region', root);
    const reg = regionEl ? regionEl.getBoundingClientRect() : null;
    let best = null, bestScore = Infinity;
    for (const e of all) {
      if (e === cur) continue;
      const q = e.getBoundingClientRect();
      if (!q.width) continue;
      const ex = q.left + q.width / 2, ey = q.top + q.height / 2;
      const dx = ex - cx, dy = ey - cy;
      let main, side;
      if (dir === 'right') { if (q.left < r.right - 4) continue; main = dx; side = dy; }
      else if (dir === 'left') { if (q.right > r.left + 4) continue; main = -dx; side = dy; }
      else if (dir === 'down') { if (q.top < r.bottom - 4) continue; main = dy; side = dx; }
      else { if (q.bottom > r.top + 4) continue; main = -dy; side = dx; }
      // Tiles slid out of a row's view don't count when coming in from above or below.
      if ((dir === 'down' || dir === 'up') && reg && (q.right < reg.left || q.left > reg.right)) continue;
      const score = main + Math.abs(side) * 2.4;
      if (score < bestScore) { bestScore = score; best = e; }
    }
    if (!best) return;
    const into = best.closest('.tv-row');
    if (into && into !== fromRow && TV.row[into.dataset.row]) {
      const back = $$('[data-f]', into).find((x) => x.dataset.key === TV.row[into.dataset.row]);
      if (back) best = back;
    }
    focus(best);
  }

  function go(view, arg, opts = {}) {
    // A sheet left open doesn't follow to the next page.
    if (TV.sheet) { TV.sheet = null; for (const sh of $$('.tv-sheet', root)) sh.remove(); }
    TV.back.push({ view: TV.view, arg: TV.arg, key: (focused() || {}).dataset?.key, y: TV.scrollY });
    if (['game', 'shots', 'mods', 'gsettings'].includes(view)) TV.from = ['game', 'shots', 'mods', 'gsettings'].includes(TV.view) ? TV.from : TV.view;
    TV.view = view; TV.arg = arg;
    TV.enter = opts.enter || (view === 'game' ? 'game' : 'in');
    TV.scrollY = 0;
    if (!opts.quiet) sfx('open');
    draw();
  }

  // Opening a game from a tile: the tile grows to fill the screen, and the page appears under it.
  function openGame(el, id) {
    const a = byId(id) || { id, name: id };
    const img = el.querySelector('img');
    const r = el.getBoundingClientRect();
    if (!img || matchMedia('(prefers-reduced-motion: reduce)').matches) return go('game', id);
    sfx('open');
    const z = document.createElement('div');
    z.className = 'tv-zoom';
    z.style.left = r.left + 'px'; z.style.top = r.top + 'px'; z.style.width = r.width + 'px'; z.style.height = r.height + 'px';
    z.innerHTML = `<img src="${esc(a.hero || a.cover ? heroOf(a) : img.src)}" alt="">`;
    root.append(z);
    void z.offsetWidth;
    z.classList.add('grow');
    Object.assign(z.style, { left: '0px', top: '0px', width: innerWidth + 'px', height: innerHeight + 'px' });
    setTimeout(() => {
      go('game', id, { enter: 'zoom', quiet: true });
      z.classList.add('done');
      setTimeout(() => z.remove(), 420);
    }, 300);
  }

  function back() {
    if (TV.sheet) { if (TV.sheet.back && TV.sheet.back()) return; return closeSheet(); }
    const b = TV.back.pop();
    if (!b) {
      if (TV.view !== 'home') { TV.view = 'home'; TV.enter = 'back'; draw(); return; }
      // Back on Home: offer the way out, like Steam (B or Esc, then A).
      return sheet(t('Leave Big Picture mode'), [
        { label: t('Leave Big Picture mode'), sub: t('Back to the desktop view, for mouse and keyboard'), icon: 'chevL', run: () => leave() },
        { label: t('Cancel'), icon: 'check', run: () => {} },
      ]);
    }
    TV.view = b.view; TV.arg = b.arg; TV.focusKey = b.key;
    TV.enter = 'back';
    draw();
    if (b.y) { setScroll(b.y, true); const f = focused(); if (f) keepInView(f, true); }
  }

  function tab(step) {
    const order = TABS.map((x) => x[0]);
    const i = order.indexOf(tabOf());
    switchTo(order[(i + step + order.length) % order.length], step > 0 ? 'right' : 'left');
  }
  function switchTo(view, enter) {
    if (TV.sheet) closeSheet();
    TV.back = [];
    TV.view = view; TV.arg = null;
    TV.enter = enter || 'in';
    TV.scrollY = 0;
    sfx('tab');
    draw();
  }

  async function activate(el) {
    if (!el || el.disabled) return;
    // With a sheet open, only what's on it can be picked.
    if (TV.sheet && !el.closest('.tv-sheet')) return;
    if (el.dataset.sheet !== undefined) { const it = TV.sheet && TV.sheet.items[Number(el.dataset.sheet)]; if (!(TV.sheet && TV.sheet.keep)) closeSheet(); TV.calm = performance.now() + 300; if (it && it.run) { sfx('select'); await it.run(); } return; }
    if (el.dataset.tab) return switchTo(el.dataset.tab, 'in');
    if (el.dataset.genre !== undefined) {
      if (TV.genre === el.dataset.genre) return;
      sfx('select');
      TV.genre = el.dataset.genre; TV.focusKey = el.dataset.key; TV.swap = true;
      const keepY = TV.scrollY;
      draw(true);
      setScroll(keepY, false);
      return;
    }
    if (el.dataset.toggle) { sfx('select'); const k = el.dataset.toggle, on = k === 'sounds' || k === 'discord' ? S.settings[k] !== false : !!S.settings[k]; await saveSettings({ [k]: !on }); return draw(true); }
    if (el.dataset.shotI !== undefined) { sfx('select'); return shotView(Number(el.dataset.shotI)); }
    if (el.dataset.gset) return cycleSetting(el, 1);
    if (el.dataset.gbmod) return tvGbMod(TV.arg, el.dataset.gbmod);
    if (el.dataset.mod) {
      const full = el.dataset.mod, have = ((S.mods[TV.arg] || {}).installed || []).some((x) => x.full === full);
      if (modBusy[full]) return;
      sfx('select');
      return have ? removeMod(TV.arg, full) : installMod(TV.arg, full);
    }
    const act = el.dataset.tvact, id = el.dataset.id;
    switch (act) {
      case 'play': sfx('select'); return playGame(id);
      case 'resume': sfx('select'); return invoke('resume_game').catch(fail);
      case 'cancel': return invoke('cancel', { id });
      case 'install': sfx('select'); return startInstall(id, S.settings.library_dir).then(() => draw(true));
      case 'page': return go('game', id);
      case 'back': return back();
      case 'leave': return leave();
      case 'lang': sfx('open'); return pickLang();
      case 'account':
        sfx('open');
        if (!(S.account && S.account.signedIn)) return window.signIn && window.signIn(false);
        return sheet(S.account.user.username, [
          { label: t('Stay signed in'), sub: t('Nothing changes'), icon: 'chevL', run: () => {} },
          { label: t('Sign out'), sub: t('Your saves stay on this PC'), icon: 'user', run: () => window.signOut && window.signOut() },
        ]);
      case 'versions': return pickVersion(id);
      case 'follow': { const d = devOf(byId(id)); if (d) { sfx('select'); await toggleFollow(d.slug); draw(true); } return; }
      case 'shots': await loadAllShots(); return go('shots', id);
      case 'mods': return go('mods', id);
      case 'gsettings': return go('gsettings', id);
      case 'pick-copy': case 'setup': sfx('select'); return tvSetup(id);
      case 'gaction': {
        const run = async () => { try { const r = await invoke('game_action', { id, action: el.dataset.k }); if (r) toast(t(r)); } catch (e) { fail(e); } };
        const a = ((S.setup[id] || {}).actions || []).find((x) => x.id === el.dataset.k);
        if (!a || a.kind !== 'clear') return run();
        return sheet(tf('Clear its {what}?', { what: a.label.toLowerCase() }), [
          { label: t('Keep it'), sub: t('Nothing changes'), icon: 'chevL', run: () => {} },
          { label: t('Clear it'), sub: a.help, icon: 'bin', run },
        ]);
      }
      case 'more': {
        const g = S.library[id];
        return sheet(g ? g.name : '', [
          ...(S.running.has(id) ? [{ label: t('Close the game'), sub: t('Anything you haven\'t saved is lost'), icon: 'stop', run: () => invoke('stop', { id }) }] : []),
          ...(S.steam ? [{ label: t('Add to Steam'), sub: t('With its art'), icon: 'steam', run: () => addToSteam(id, false) }] : []),
          ...(g && S.account && S.account.signedIn ? [{ label: g.cloud ? t('Turn off cloud saves') : t('Turn on cloud saves'), sub: g.cloud ? t('Its saves stay on this PC from now on') : t('Its saves go to your account after you play'), icon: 'cloud', run: async () => { try { S.library[id] = await invoke('saves_cloud', { id, on: !g.cloud }); toast(g.cloud ? t('Cloud saves are off for it. What\'s in the cloud stays there.') : t('Cloud saves are on for it.')); } catch (e) { fail(e); } } }] : []),
          { label: t('Add to desktop'), sub: t('A shortcut that starts it'), icon: 'desk', run: async () => { try { await invoke('desktop_shortcut', { id }); toast(tf('{name} is on your desktop.', { name: g.name })); } catch (e) { fail(e); } } },
          ...(window.tvBetaItem && window.tvBetaItem(id) ? [window.tvBetaItem(id)] : []),
          { label: t('Older releases'), sub: t('Go back to an older one, or forward again'), icon: 'history', run: () => versionsSheet(id) },
          { label: t('Open its folder'), sub: g ? g.path : '', icon: 'folder', run: () => invoke('open_folder', { id }).catch(fail) },
        ]);
      }
      case 'size': {
        sfx('select');
        const order = ['', 'small', 'medium', 'large'];
        await saveSettings({ tv_size: order[(order.indexOf(S.settings.tv_size || '') + 1) % order.length] });
        TV.pill = null;
        return draw(true);
      }
    }
    if (el.dataset.goGame) {
      // A tile grows into the page; buttons and list items just go there.
      if (el.matches('.tv-poster, .tv-card')) return openGame(el, el.dataset.goGame);
      return go('game', el.dataset.goGame);
    }
  }

  function press(name) {
    if (!TV.on) return false;
    // A dialog on top (Restart Steam, Switch games): A picks its main button, B closes it.
    const dlg = $('.scrim');
    if (dlg) {
      if (name === 'b') dlg.remove();
      else if (name === 'a') { const b = $('[data-x="yes"], .btn.main', dlg) || $('[data-x]', dlg); if (b) b.click(); }
      return true;
    }
    if ($('.tv-boot', root)) return true;
    if (TV.shot != null && $('.tv-viewer', root)) { shotAct(name); return true; }
    // Just after picking from a sheet, the page under it is changing: a press then is the same
    // press bouncing, not a new one.
    if (TV.calm && performance.now() < TV.calm && !TV.sheet && ['a', 'x', 'y'].includes(name)) return true;
    const cur = focused();
    if (TV.sheet && TV.sheet.keys && TV.sheet.keys(name, cur)) return true;
    switch (name) {
      case 'left': case 'right': if (cur && cur.dataset.gset) { cycleSetting(cur, name === 'right' ? 1 : -1); break; }
      // falls through
      case 'up': case 'down': { const before = focused(); move(name); if (focused() !== before) sfx('move'); break; }
      case 'a': activate(cur); break;
      case 'b': if (TV.sheet || TV.back.length || TV.view !== 'home') sfx('back'); back(); break;
      case 'x': if (TV.view === 'mods') { const mm = S.mods[TV.arg]; if (mm && mm.community && mm.gamebanana) { S.modSrc[TV.arg] = (S.modSrc[TV.arg] || 'ts') === 'ts' ? 'gb' : 'ts'; sfx('move'); draw(true); } break; }
        if (TV.view === 'shots' && cur && cur.dataset.shotI !== undefined) { TV.shot = Number(cur.dataset.shotI); shotAct('x'); break; }
        if (TV.sheet && TV.sheet.vers) { sfx('back'); closeSheet(); break; }
        if (TV.view === 'game' && !TV.sheet && verGroup(byId(TV.arg))) { pickVersion(TV.arg); break; }
        break;
      case 'y': { const id = cur && (cur.dataset.hero || cur.dataset.goGame || (cur.closest('.tv-hero') && TV.hero)); if (id && TV.view !== 'game') go('game', id); break; }
      case 'lb': tab(-1); break;
      case 'rb': tab(1); break;
      case 'menu': switchTo('settings'); break;
      default: return false;
    }
    return true;
  }

  // Controllers: from the core (Xbox through XInput, PlayStation read directly), with the
  // window's own gamepad support as a fallback if the core never reports anything.
  let nativeSeen = false;
  let lastPad = { b: '', t: 0 };
  const BORN = performance.now();
  function pad(b, kind) {
    // The same button twice within 70 ms is one press arriving twice (a PlayStation controller
    // read directly and again through Steam Input): only the first counts, and the button icons
    // don't flip between the two.
    const at = performance.now();
    if (b !== 'menu-hold' && b === lastPad.b && at - lastPad.t < 70) return;
    if (b !== 'menu-hold') lastPad = { b, t: at };
    if (kind && kind !== S.padKind) {
      S.padKind = kind;
      if (TV.on && !$('.tv-boot', root)) draw(true); // the button icons follow the controller
    }
    if (!TV.on) {
      // The file browser and the first-run setup take the controller while they're up.
      if (window.fbPad && window.fbPad(b)) return;
      if (window.oobPad && window.oobPad(b)) return;
      // A controller on the desktop view: holding Menu (Options) for a second opens TV mode. Not in
      // the first few seconds after the launcher opens, and not while the first-run setup is up.
      if (b === 'menu-hold' && document.hasFocus() && performance.now() - BORN > 3000 && !document.querySelector('.oob')) enter();
      else if (!TV.hinted && ['a', 'b', 'up', 'down', 'left', 'right', 'menu'].includes(b) && !document.querySelector('.oob')) { TV.hinted = true; if (window.toast) toast(t('Hold Menu on your controller to switch to Big Picture mode.')); }
      return;
    }
    // Holding Menu (Options) for a second goes back to the desktop view, the way it came in.
    if (b === 'menu-hold') { if (!TV.sheet && performance.now() - (TV.enteredAt || 0) > 2000) leave(); return; }
    root.classList.add('mouse-off');
    press(b);
  }
  T.event.listen('pad', ({ payload }) => {
    nativeSeen = true;
    if (typeof payload === 'string') pad(payload); else pad(payload.b, payload.kind);
  });
  const STD = { 0: 'a', 1: 'b', 2: 'x', 3: 'y', 4: 'lb', 5: 'rb', 8: 'view', 9: 'menu', 12: 'up', 13: 'down', 14: 'left', 15: 'right', 16: 'guide' };
  const held = {};
  const menuAt = {};
  let rep = { b: null, t: 0, n: 0 };
  // A backup for when the launcher's own controller reading isn't there. A timer, not every frame,
  // so an idle window doesn't keep drawing; and it stops for good once the launcher's own works.
  const rested = {};
  function pollPads() {
    if (nativeSeen || !navigator.getGamepads) return;
    setTimeout(pollPads, document.hasFocus() && !S.away ? 33 : 500);
    if (!document.hasFocus() || S.away) return;
    for (const gp of navigator.getGamepads()) {
      if (!gp) continue;
      const kind = /054c|sony|dualsense|playstation|wireless controller/i.test(gp.id) ? 'ps' : 'xbox';
      const now = {};
      gp.buttons.forEach((bt, i) => { if (STD[i] && bt.pressed) now[STD[i]] = 1; });
      const [ax, ay] = [gp.axes[0] || 0, gp.axes[1] || 0];
      // The stick counts once it has been seen resting near the middle (a wheel or a worn stick
      // that sits pushed all the way doesn't scroll on its own).
      const rk = gp.index + ':' + gp.id;
      if (Math.abs(ax) < 0.3 && Math.abs(ay) < 0.3) rested[rk] = true;
      if (rested[rk]) { if (ax < -0.6) now.left = 1; if (ax > 0.6) now.right = 1; if (ay < -0.6) now.up = 1; if (ay > 0.6) now.down = 1; }
      const k = gp.index;
      held[k] = held[k] || {};
      for (const b of Object.keys(now)) if (!held[k][b]) { pad(b, kind); if (['up', 'down', 'left', 'right'].includes(b)) rep = { b, t: performance.now(), n: 0 }; if (b === 'menu') menuAt[k] = performance.now(); }
      if (!now.menu) delete menuAt[k];
      else if (menuAt[k] && performance.now() - menuAt[k] > 900) { delete menuAt[k]; pad('menu-hold', kind); }
      if (rep.b && now[rep.b] && performance.now() - rep.t > 400 + rep.n * 120) { rep.n++; pad(rep.b, kind); }
      if (rep.b && !now[rep.b]) rep.b = null;
      held[k] = now;
    }
  }
  pollPads();

  // F11 switches between the desktop view and Big Picture mode.
  document.addEventListener('keydown', (e) => {
    if (e.key !== 'F11' || e.repeat || document.querySelector('.oob')) return;
    e.preventDefault();
    if (TV.on) leave(); else enter();
  }, true);
  document.addEventListener('keydown', (e) => {
    if (!TV.on) return;
    const map = { ArrowUp: 'up', ArrowDown: 'down', ArrowLeft: 'left', ArrowRight: 'right', Enter: 'a', ' ': 'a', Escape: 'b', Backspace: 'b', q: 'lb', e: 'rb', PageUp: 'lb', PageDown: 'rb', x: 'x', y: 'y', F10: 'menu' };
    const n = map[e.key];
    if (!n) return;
    e.preventDefault();
    e.stopPropagation();
    root.classList.add('mouse-off');
    press(n);
  }, true);

  root.addEventListener('mousemove', () => root.classList.remove('mouse-off'));
  root.addEventListener('click', (e) => {
    const el = e.target.closest('[data-f]');
    if (!el) return;
    e.stopPropagation();
    if (!el.classList.contains('f')) focus(el);
    activate(el);
  });

  // Keep the pages current while things happen elsewhere.
  let redraw = 0;
  const soon = () => { if (!TV.on || $('.tv-boot', root)) return; clearTimeout(redraw); redraw = setTimeout(() => draw(true), 120); };
  T.event.listen('install', ({ payload: p }) => { if (['done', 'error', 'cancelled', 'prepare', 'queued', 'unpack'].includes(p.phase)) setTimeout(soon, 300); });
  T.event.listen('game', () => setTimeout(soon, 300));
  T.event.listen('screenshot', () => setTimeout(soon, 400));
  setInterval(() => { const c = $('#tv-clock', root); if (c) c.textContent = clock(); }, 20000);
})();
