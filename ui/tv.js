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
  const TV = { on: false, view: 'home', arg: null, back: [], hero: null, row: {}, focusKey: null, genre: '', shot: null, sheet: null, sort: null, dl: {}, saves: {}, sizes: null, savesAll: null };
  // Pages that belong to one game (they have a Back pill instead of the sections).
  const GAMEVIEWS = ['game', 'shots', 'mods', 'gsettings', 'gsaves'];
  const TABS = [['home', N_('Home')], ['library', N_('Library')], ['achievements', N_('Achievements')], ['discover', N_('Discover')], ['downloads', N_('Downloads')], ['settings', N_('Settings')]];
  // The tabs in the order the player gave the sidebar, without the ones they hid; Settings last.
  const tvTabs = () => {
    const ui = (S.settings && S.settings.ui) || {}, order = ui.side_order || [], hide = new Set(ui.side_hide || []);
    const rank = (k) => (k === 'settings' ? 999 : order.indexOf(k) >= 0 ? order.indexOf(k) : 100 + TABS.findIndex((x) => x[0] === k));
    return TABS.filter(([k]) => k === 'settings' || k === 'home' || k === 'library' || !hide.has(k)).sort((a, b) => rank(a[0]) - rank(b[0]));
  };
  const SX = 96; // the side margin, in stage pixels
  const MARK = '<svg viewBox="0 0 256 256" aria-hidden="true"><defs><linearGradient id="tvm" x1="0" y1="38" x2="0" y2="218" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="#34f2e8"/><stop offset="1" stop-color="#377bb6"/></linearGradient></defs><g fill="url(#tvm)" transform="translate(4 -2)"><path d="M40 40H88V168H116L64 220L12 168H40Z"/><path d="M104 40H172L216 84V116L180 152H104V116H164L180 100V92L164 76H104Z"/><path d="M132 168H184L236 220H184Z"/></g></svg>';
  const ICON = {
    plus: '<path d="M12 5v14M5 12h14"/>',
    search: '<circle cx="11" cy="11" r="6.5"/><path d="m16 16 4.5 4.5"/>',
    filter: '<path d="M4 6h16M7 12h10M10 18h4"/>',
    erase: '<path d="M9 6h11v12H9l-5-6z"/><path d="m12 10 4 4m0-4-4 4"/>',
    wrench: '<path d="M14.7 6.3a4 4 0 0 0 5 5L21 13l-8 8-3-3 8-8-1.3-1.3a4 4 0 0 1-5-5L9 2l3 3-2 2-3-3L5.3 5.7"/>',
    cloud: '<path d="M7 18a4.5 4.5 0 0 1-.6-9 6 6 0 0 1 11.5 1.6A3.8 3.8 0 0 1 17.5 18z"/>',
    user: '<circle cx="12" cy="8.5" r="3.6"/><path d="M4.8 20a7.2 7.2 0 0 1 14.4 0"/>',
    check: '<path d="m5 12.5 4.5 4.5L19 7.5"/>',
    close: '<path d="m6 6 12 12M18 6 6 18"/>',
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
    play: '<path d="M8 5.5v13l10.5-6.5z" fill="currentColor"/>',
    pin: '<path d="M9 4h6l-1 5 3 3v2H7v-2l3-3z"/><path d="M12 14v6"/>',
    page: '<rect x="4" y="4" width="16" height="16" rx="3"/><path d="M8 9h8M8 13h8M8 17h5"/>',
    save: '<path d="M7 3h10a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z"/><path d="M9 3v3.5M12 3v3.5M15 3v3.5M8.5 12h7v5h-7z"/>',
    chevD: '<path d="m5 9 7 7 7-7"/>',
    move: '<path d="m8 8-4 4 4 4M16 8l4 4-4 4M4 12h16"/>',
    drive: '<rect x="3" y="6" width="18" height="12" rx="2.5"/><path d="M7 14h.01M11 14h6"/>',
    upload: '<path d="M12 20V9m0 0-4.5 4.5M12 9l4.5 4.5M5 4h14"/>',
    down: '<path d="M12 4v11m0 0-4.5-4.5M12 15l4.5-4.5M5 20h14"/>',
    pad: '<path d="M7.5 7h9a5 5 0 0 1 4.8 6.4l-1 3.4a2.6 2.6 0 0 1-4.4 1L14 16h-4l-1.9 1.8a2.6 2.6 0 0 1-4.4-1l-1-3.4A5 5 0 0 1 7.5 7z"/><path d="M8 10.5v3M6.5 12h3"/><circle cx="16" cy="11" r=".9" fill="currentColor"/><circle cx="17.6" cy="13" r=".9" fill="currentColor"/>',
  };
  const svg = (k, s = 26) => `<svg width="${s}" height="${s}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${ICON[k]}</svg>`;

  // Button icons for the controller in use: Xbox letters, or PlayStation shapes.
  const glyph = (n, cls = '') => {
    const pref = (S.settings.ui || {}).tv_glyphs;
    const p = padArt(pref || S.padKind || 'xbox', n);
    return `<i class="glyph g-${n} ${cls}" style="--m:url(${p.url})" role="img" aria-label="${esc(t(p.name))}"></i>`;
  };

  // ---------- sounds ----------
  // Start-up, moving, going back and changing section are RecompStore's. Picking something and
  // opening a game are soft, short tones made here.
  const SFX = {};
  let ctx = null;
  function tone(kind) {
    if (S.appimage) return;
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
    if (S.settings.sounds === false || S.appimage) return;
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
  const stage = () => $('.tv-stage:not(.leaving)', root);
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
    root.style.setProperty('--k', k);
  }
  addEventListener('resize', () => { fit(); if (TV.on && !TV.booting && !$('.tv-boot', root)) draw(true); });

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

  // Into Big Picture: the launcher fades to dark first, the window goes full screen behind the
  // dark, and only then the start-up begins, so the resize never shows.
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  async function enter() {
    if (TV.on) return;
    TV.on = true;
    TV.booting = true;
    TV.enteredAt = performance.now();
    root.innerHTML = '';
    root.hidden = false;
    root.classList.add('tv-cover');
    await wait(still() ? 0 : 240);
    document.body.classList.add('tv-on');
    try { await W.setFullscreen(true); } catch (e) { /* the window can stay as it is */ }
    await wait(160);
    root.classList.remove('tv-cover');
    boot();
  }
  async function leave() {
    root.classList.add('tv-cover', 'out');
    await wait(still() ? 0 : 220);
    TV.on = false;
    root.innerHTML = '';
    document.body.classList.remove('tv-on');
    try { await W.setFullscreen(false); } catch (e) { /* fine */ }
    render(true);
    // Pictures left black by the switch out of full screen: shown or loaded again, now and once the
    // window has settled at its size.
    requestAnimationFrame(() => window.reviveImages && window.reviveImages());
    setTimeout(() => window.reviveImages && window.reviveImages(), 700);
    setTimeout(() => window.reviveImages && window.reviveImages(), 1800);
    await wait(120);
    // Then the dark lifts off the launcher.
    root.style.transition = 'opacity .22s ease';
    root.style.opacity = '0';
    await wait(still() ? 0 : 230);
    root.hidden = true;
    root.classList.remove('tv-cover', 'out');
    root.style.transition = '';
    root.style.opacity = '';
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
      setTimeout(() => { if (!TV.on) return; TV.booting = false; TV.view = (S.settings.ui || {}).start_page === 'library' && Object.keys(S.library).length ? 'library' : 'home'; TV.back = []; TV.enter = 'boot'; draw(); if (!S.settings.data_asked && S.settings.setup_done) setTimeout(() => window.tvAskData(), 700); }, 420);
    }, 3100);
  }

  // ---------- pieces ----------
  const heroOf = (a) => (a ? media(a.hero || a.cover) : '');
  // ---------- colour from the cover ----------
  // Each game's cover gives the page its colours: a light accent (buttons, the line over the
  // title, the glow) and a deep shade that tints the backdrop. Worked out once per cover from a
  // tiny copy of it.
  const TONES = {};
  const lighter = () => document.body.classList.contains('lighter');
  function toneOf(a) {
    if (!a || !a.cover) return Promise.resolve(null);
    if (TONES[a.id]) return TONES[a.id];
    TONES[a.id] = new Promise((res) => {
      const img = new Image();
      img.crossOrigin = 'anonymous';
      img.onload = () => {
        try {
          const N = 28, c = document.createElement('canvas');
          c.width = c.height = N;
          const x = c.getContext('2d', { willReadFrequently: true });
          x.drawImage(img, 0, 0, N, N);
          const d = x.getImageData(0, 0, N, N).data;
          // Colours sorted into twelve hues, each counted by how vivid it is; the strongest hue wins.
          const bins = Array.from({ length: 12 }, () => ({ w: 0, r: 0, g: 0, b: 0 }));
          let all = 0, lum = 0;
          for (let i = 0; i < d.length; i += 4) lum += (0.2126 * d[i] + 0.7152 * d[i + 1] + 0.0722 * d[i + 2]) / 255;
          lum /= d.length / 4;
          for (let i = 0; i < d.length; i += 4) {
            const r = d[i] / 255, g = d[i + 1] / 255, b = d[i + 2] / 255;
            const mx = Math.max(r, g, b), mn = Math.min(r, g, b), ch = mx - mn;
            if (ch < 0.08 || mx < 0.15) continue;
            let h = mx === r ? ((g - b) / ch) % 6 : mx === g ? (b - r) / ch + 2 : (r - g) / ch + 4;
            h = (h * 60 + 360) % 360;
            const w = ch * ch * mx, bin = bins[Math.floor(h / 30) % 12];
            bin.w += w; bin.r += r * w; bin.g += g * w; bin.b += b * w; all += w;
          }
          const best = bins.reduce((p, q) => (q.w > p.w ? q : p));
          if (!best.w || all < 2) return res({ h: 215, s: 12, l: lum, grey: true });
          const r = best.r / best.w, g = best.g / best.w, b = best.b / best.w;
          const mx = Math.max(r, g, b), mn = Math.min(r, g, b), l = (mx + mn) / 2, ch = mx - mn;
          let h = mx === r ? ((g - b) / ch) % 6 : mx === g ? (b - r) / ch + 2 : (r - g) / ch + 4;
          h = Math.round((h * 60 + 360) % 360);
          const sat = ch / (1 - Math.abs(2 * l - 1) || 1);
          const S1 = Math.round(Math.min(88, Math.max(48, sat * 100)));
          res({ h, s: S1, l: lum });
        } catch (e) { res(null); }
      };
      img.onerror = () => res(null);
      img.src = media(a.cover);
    });
    return TONES[a.id];
  }
  // The colours for the game in view (none: the launcher's own).
  function setTone(id) {
    TV.toneWant = id || null;
    const paint = (tn) => {
      if (TV.toneWant !== (id || null)) return;
      const light = document.body.classList.contains('light');
      if (!tn) { root.style.removeProperty('--acc'); root.style.removeProperty('--accd'); root.style.removeProperty('--accink'); return; }
      root.style.setProperty('--acc', light ? `hsl(${tn.h} ${Math.min(tn.s, 75)}% 38%)` : `hsl(${tn.h} ${tn.s}% 64%)`);
      root.style.setProperty('--accd', light ? `hsl(${tn.h} 40% 90%)` : `hsl(${tn.h} ${Math.round(tn.s * 0.55)}% 13%)`);
      root.style.setProperty('--accink', light ? '#FFFFFF' : '#08101C');
    };
    if (!id) return paint(null);
    const a = byId(id) || (S.library[id] && libApp(S.library[id]));
    toneOf(a).then((tn) => paint(tn && !tn.grey ? tn : null));
  }
  const libAppOf = (id) => byId(id) || (S.library[id] ? libApp(S.library[id]) : { id, name: id });
  // A game's name as its logo when it has one, else as text.
  const titleOf = (a, cls = 'tv-h1') => (a.logo ? `<img class="tv-logo" src="${esc(media(a.logo))}" alt="${esc(a.name)}" data-fallback="${cls}">` : `<h1 class="${cls}">${esc(a.name)}</h1>`);
  // Pinned games (Settings: Home Shows Pinned Games).
  const pins = () => ((S.settings.ui || {}).tv_pins || []).filter((id) => byId(id) || S.library[id]);
  const homePinned = () => (S.settings.ui || {}).tv_home === 'pinned';
  const installed = () => Object.values(S.library).sort((x, y) => (y.last_played || 0) - (x.last_played || 0) || (y.installed_at || 0) - (x.installed_at || 0)).map((g) => ({ g, a: libApp(g) }));
  const pool = () => visible().filter((a) => forOS(a) && !a.soon);
  const timeLine = (g) => (!g ? '' : !g.last_played ? t('Not played yet') : `${ago(g.last_played).replace(/^./, (c) => c.toUpperCase())} · ${playedFor(g)}`);

  const htmlIn = (text, v) => esc(text).replace(/\{(\w+)\}/g, (m, k) => (k in v ? v[k] : m));
  // A wide card (Continue playing, What's new).
  function card(a, key, sub) {
    const g = S.library[a.id];
    return `<button type="button" class="tv-card" data-f data-key="${key}" data-hero="${esc(a.id)}" data-go-game="${esc(a.id)}">
      <span class="ph">${esc(a.name)}</span>${a.hero || a.cover ? `<img src="${esc(heroOf(a))}" alt="">` : ''}
      ${S.running.has(a.id) ? `<span class="tv-badge">${esc(t('Playing'))}</span>` : S.updates[a.id] ? `<span class="tv-badge">${esc(t('Update'))}</span>` : sreqBadge(a)}
      <span class="cap"><b>${esc(a.name)}</b><span>${esc(sub != null ? sub : g ? timeLine(g) : a.game || '')}</span></span></button>`;
  }

  // A cover, with its name under it.
  function poster(a, key, sub) {
    const st = status(a), j = S.jobs[a.id];
    const badge = st === 'update' ? `<span class="tv-badge">${esc(t('Update'))}</span>` : st === 'running' ? `<span class="tv-badge">${esc(t('Playing'))}</span>` : st === 'soon' ? `<span class="tv-badge soon">${esc(t('Soon'))}</span>` : sreqBadge(a) || (st === 'installed' && TV.view !== 'library' && !String(key).startsWith('c:') ? `<span class="tv-badge left">${esc(t('Installed'))}</span>` : '');
    return `<div class="tv-cell">${a.cover ? `<span class="aura" style="background-image:url('${cssUrl(media(a.cover))}')"></span>` : ''}<button type="button" class="tv-poster" data-f data-key="${key}" data-hero="${esc(a.id)}" data-go-game="${esc(a.id)}" aria-label="${esc(a.name)}">
      <span class="ph">${esc(a.name)}</span>${a.cover ? `<img src="${esc(media(a.cover))}" alt="" loading="lazy">` : ''}${badge}${TV.view !== 'library' && !j && verGroup(a) ? `<span class="tv-vers">${svg('layers', 18)}${fmtNum(verGroup(a).list.length)}</span>` : ''}${j ? `<span class="tv-pbar"><i data-prog-bar="${esc(a.id)}" style="width:${pct(j)}%"></i></span>` : ''}</button>
      <span class="tv-name">${esc(a.name)}${(devOf(a) || {}).verified ? TICK(18) : ''}</span>${sub ? `<span class="tv-sub">${esc(sub)}</span>` : ''}</div>`;
  }

  const btn = (key, act, label, { cls = '', icon = '', id = '', g = '' } = {}) => `<button type="button" class="tv-btn ${cls}" data-f data-key="${key}" data-tvact="${act}" data-id="${esc(id)}">${g ? glyph(g, 'tv-keyg') : icon ? svg(icon) : ''}<span>${label}</span></button>`;

  // The main button: Play, Install, Update, a download's progress, or Back to the game.
  // Setup Required: a yellow badge on the cover until the player says it's set up.
  const sreqBadge = (a) => needsSetup(a) && !a.soon && !setupDone(a.id) ? `<span class="tv-badge sreq">${svg('wrench', 16)}${esc(t('Setup Required'))}</span>` : '';
  // What a Setup Required recomp needs before it plays, as a sheet: its developer's steps, the
  // README, its folder, and Done, Play (after which Play is back for good).
  function tvSetupReq(id) {
    const a = byId(id);
    if (!a) return;
    const inst = S.library[id];
    const steps = (a.setup_steps || []).filter(Boolean);
    const readme = a.setup_readme || (a.repo ? `https://github.com/${a.repo}#readme` : '');
    const head = `<h3>${esc(tf('Set up {name}', { name: a.name }))}</h3><p class="tv-sreq-p">${esc(t('This one needs a few things on your PC before it plays. Do these once, in order:'))}</p>
      ${steps.length ? `<ol class="tv-sreq-steps">${steps.map((x) => `<li dir="auto">${esc(x)}</li>`).join('')}</ol>` : `<p class="tv-sreq-p">${esc(t('Its developer\'s README has the steps.'))}</p>`}
      ${inst ? '' : `<p class="tv-sreq-p dim">${esc(t('Install it first, then come back here.'))}</p>`}`;
    const items = [readme ? { label: t('Open the README'), icon: 'page', run: () => invoke('open_url', { url: readme }).catch(fail) } : null];
    if (inst) items.push({ label: t('Open its folder'), icon: 'folder', run: () => invoke('open_folder', { id }).catch(fail) },
      { label: t('Done, Play'), icon: 'play', cls: 'warn', run: async () => { await saveUi({ setup_ok: Object.assign({}, (S.settings.ui || {}).setup_ok || {}, { [id]: true }) }, true); draw(true); playGame(id); } },
      { label: t('Not yet'), icon: 'chevL', run: () => {} });
    else items.push({ label: t('Install'), sub: a.size || '', icon: 'down', run: () => startInstall(id, S.settings.library_dir).then(() => draw(true)) }, { label: t('Close'), icon: 'chevL', run: () => {} });
    sheet(tf('Set up {name}', { name: a.name }), items.filter(Boolean), 'sreq', head);
  }
  // A game whose folder isn't there (app.js missingModal), as a sheet for the controller.
  window.tvMissing = (id) => {
    if (!TV.on) return false;
    const g = S.library[id];
    if (!g) return true;
    const a = libAppOf(id);
    const head = `<h3>${esc(tf('Can\'t Find {name}', { name: a.name }))}</h3><p class="tv-sh-p">${esc(t('Its folder isn\'t where it was. If it\'s on a drive that isn\'t connected, connect it and try again.'))}</p><p class="tv-sh-p" dir="ltr">${esc(g.path)}</p>`;
    sheet('', [
      { label: t('Try Again'), icon: 'refresh', run: async () => { await refreshState(); draw(true); if (S.missing.has(id)) window.tvMissing(id); else playGame(id); } },
      { label: t('Find Its Folder…'), icon: 'folder', run: async () => {
        const p = await pickFile({ title: tf('Where {name} is now', { name: a.name }), directory: true }).catch(fail);
        if (!p) return;
        try { S.library[id] = await invoke('locate_game', { id, path: typeof p === 'string' ? p : p.path }); await refreshState(); draw(true); toast(tf('Found {name}.', { name: a.name })); } catch (e) { fail(e); }
      } },
      { label: t('Remove from Library'), icon: 'bin', run: async () => { try { await invoke('forget_game', { id }); await refreshState(); draw(true); } catch (e) { fail(e); } } },
    ], '', head);
    return true;
  };
  // Sharing, asked once of players who had the launcher before it was asked (app.js askData).
  window.tvAskData = () => {
    if (!TV.on) return false;
    if (TV.booting || TV.flying || !stage() || TV.sheet) { setTimeout(() => window.tvAskData(), 400); return true; }
    if (S.settings.data_asked) return true;
    const set = async (u, s) => { await saveSettings({ data_asked: true, share_usage: u, share_stats: s }, true); };
    const head = `<h3>${esc(t('Help Improve RecompLauncher?'))}</h3><p class="tv-sh-p">${esc(t('RecompLauncher can send a little information to help fix problems and see what players use. Nothing is sent unless you say so, and you can change this any time in Settings.'))}</p>`;
    sheet('', [
      { label: t('Don\'t Share'), sub: t('Nothing is sent'), icon: 'close', run: () => set(false, false) },
      { label: t('Share Anonymous Stats'), sub: t('When the launcher is open and which recomps are played, under a random ID'), icon: 'check', run: () => set(false, true) },
      { label: t('Share Both'), sub: t('Also your PC\'s parts and what went wrong when a game crashes. No names, no files.'), icon: 'check', run: () => set(true, true) },
    ], 'tv-ask', head);
    return true;
  };
  window.tvSetupReq = (id) => {
    if (!TV.on) return false;
    if (TV.booting || TV.flying || !stage()) { setTimeout(() => window.tvSetupReq(id), 300); return true; }
    tvSetupReq(id);
    return true;
  };
  function mainButton(a, key) {
    const st = status(a), id = a.id;
    switch (st) {
      case 'running': return btn(key, 'resume', esc(t('Back to the game')), { cls: 'main', id, g: 'a' });
      case 'settingup': return `<button type="button" class="tv-btn" data-f data-key="${key}" disabled><span>${esc(t('Its setup is open'))}</span></button>`;
      case 'busy': return `<button type="button" class="tv-btn wide" data-f data-key="${key}" data-tvact="cancel" data-id="${esc(id)}">${glyph('a', 'tv-keyg')}${progress(id)}</button>`;
      case 'update': return btn(key, 'install', esc(t('Update now')), { cls: 'main', id, g: 'a' });
      case 'installed': { const su = S.setup[id]; if (needsSetup(a) && !setupDone(id)) return btn(key, 'setupreq', esc(t('Set Up')), { cls: 'main warn', id, g: 'a' }); return S.easy.has(id) && su && su !== 'loading' && su.needs && !su.copy ? btn(key, 'setup', esc(t('Set up')), { cls: 'main', id, g: 'a' }) : btn(key, 'play', esc(isServer(byId(id) || { id }) ? t('Start Server') : t('Play')), { cls: 'main', id, g: 'a' }); }
      case 'soon': return `<button type="button" class="tv-btn" data-f data-key="${key}" disabled><span>${esc(t('Coming soon'))}</span></button>`;
      case 'nobuild': return `<button type="button" class="tv-btn" data-f data-key="${key}" disabled><span>${esc(tf('No {os} version yet', { os: OS_NAME[S.os] }))}</span></button>`;
      default: return isSaved(id) ? btn(key, 'install', esc(t('Install')) + (a.size ? ` <small>${esc(a.size)}</small>` : ''), { cls: 'main', id, g: 'a' }) : btn(key, 'addlib', esc(t('Add to Library')), { cls: 'main', id, g: 'a' });
    }
  }

  const updatesReady = () => Object.keys(S.updates || {}).filter((id) => S.library[id] && !S.jobs[id]).length + (S.launcherUpd ? 1 : 0);
  const tabOf = () => (['about', 'saves', 'storage'].includes(TV.view) ? 'settings' : GAMEVIEWS.includes(TV.view) ? TV.from || 'home' : TV.view);
  const clock = () => new Date().toLocaleTimeString(curLocale(), { hour: 'numeric', minute: '2-digit' });
  const bar = () => `<header class="tv-bar"><div class="tv-brand">${MARK}<img class="tv-wm" src="img/${document.body.classList.contains('light') ? 'wordmark-dark' : 'wordmark'}.svg" alt="RecompLauncher"></div>
    <nav class="tv-nav" aria-label="${esc(t('Sections'))}">${glyph('lb', 'tv-bumpg')}<span class="tv-tab-pill" aria-hidden="true"></span>${tvTabs().map(([k, n]) => `<button type="button" class="tv-tab${tabOf() === k ? ' on' : ''}" data-f data-key="tab:${k}" data-tab="${k}">${esc(t(n))}${k === 'downloads' && updatesReady() ? `<b class="tv-tab-n">${updatesReady()}</b>` : ''}</button>`).join('')}${glyph('rb', 'tv-bumpg')}</nav>
    <div class="tv-status"><span class="tv-clock" id="tv-clock">${clock()}</span></div></header>`;
  // The game page has a Back pill instead of the sections.
  const gameBar = () => `<header class="tv-bar"><button type="button" class="tv-back" data-f data-key="back" data-tvact="back">${glyph('b', 'tv-keyg')}<span>${esc(t('Back'))}</span></button><span></span>
    <div class="tv-status"><span class="tv-clock" id="tv-clock">${clock()}</span></div></header>`;

  function hints(list) {
    const g = (k) => k.toLowerCase().split('+').map((n) => glyph(n)).join('<em>+</em>');
    const playing = S.running.size > 0;
    return `<footer class="tv-hints">${list.map(([k, l]) => `<span>${g(k)}${esc(l)}</span>`).join('')}${playing ? `<span class="end">${glyph('view')}${esc(t('Hold for the quick menu'))}</span>` : ''}</footer>`;
  }

  const region = (inner, cls = '') => `<main class="tv-region ${cls}"><div class="tv-scroll" id="tv-scroll">${inner}</div></main>`;
  // The game's art, blurred soft (most of it is small), washed with the cover's colours.
  const bgMode = () => (S.settings.ui || {}).tv_bg || '';
  // Home and Cover Flow show the launcher's own backdrop (lit in the game's colours) unless the
  // player picked the game's art; other pages always show the launcher's, game pages the game's.
  const backdrop = (a, cls = '') => ((cls === 'home' || cls === 'flowbg') && bgMode() !== 'art') || (!a && cls !== 'flowbg')
    ? `<div class="tv-full amb ${cls}" id="tv-bg"></div>`
    : `<div class="tv-full ${cls}" id="tv-bg">${a && (a.hero || a.cover) ? `<img src="${esc(heroOf(a))}" alt="">` : '<div class="tv-glow"></div>'}<i class="tv-bokeh" aria-hidden="true"></i></div>`;
  // `raw`: the title is HTML already (with a tag after it).
  const pageHead = (title, sub, raw) => `<div class="tv-head"><h2>${raw ? title : esc(title)}</h2>${sub ? `<p>${esc(sub)}</p>` : ''}</div>`;
  const row = (name, title, items) => `<section class="tv-sec" data-sec="${name}"><h3>${esc(title)}</h3><div class="tv-rowview"><div class="tv-row" data-row="${name}">${items}</div></div></section>`;

  // ---------- pages ----------
  // Home's dock: your pinned games (Settings), else the ones you played last, all in one row.
  const DOCK_MAX = 12;
  function dockList() {
    if (homePinned()) { const p = pins().map(libAppOf); if (p.length) return { list: p, kind: 'pinned' }; }
    const mine = installed().map((x) => x.a);
    if (mine.length) return { list: mine.slice(0, DOCK_MAX), kind: 'mine' };
    // Nothing installed yet: the games picked for the top of Home, in their order.
    const picked = window.bannerApps && bannerApps();
    return picked ? { list: picked, kind: 'banner' } : { list: oneEach(pool().sort(byDl)).slice(0, 10), kind: 'top' };
  }
  function pageHome() {
    const d = dockList(), list = d.list;
    const sel = list.find((a) => a.id === TV.hero) || (TV.heroBelow && TV.hero ? libAppOf(TV.hero) : null) || list[0];
    TV.hero = sel ? sel.id : null;
    const rows = [];
    // Below the dock, a press of down away: the rest of Home.
    if (d.kind === 'pinned') { const mine = installed().map((x) => x.a); if (mine.length) rows.push(row('cont', t('Continue playing'), mine.slice(0, 14).map((a) => poster(a, 'c2:' + a.id, timeLine(S.library[a.id]))).join(''))); }
    const owned = oneEach(Object.keys(S.owned).filter((id) => !S.library[id]).map(byId).filter((a) => a && !a.soon && forOS(a) && !(S.settings.hide_ai && isAI(a)) && !haveAny(a)).sort(byDl));
    if (owned.length) rows.push(row('own', t('Found in your folders'), owned.map((a) => poster(a, 'o:' + a.id, t('You have it'))).join('')));
    const disc = oneEach(pool().filter((a) => S.easy.has(a.id) && !haveAny(a)));
    if (disc.length >= 2) rows.push(row('disc', t('Drop in your disc'), disc.map((a) => poster(a, 'e:' + a.id, t('Easy Setup'))).join('')));
    const news = (S.news || []).filter((x) => byId(x.id) || S.library[x.id]).slice(0, 8);
    if (news.length) rows.push(row('news', t('What\'s new for your games'), news.map((x) => card(byId(x.id) || { id: x.id, name: x.name }, `w:${x.id}:${x.tag}`, `${x.title.startsWith(x.name) ? x.tag : x.title} · ${date(x.date)}`)).join('')));
    const upd = recentlyUpdated(pool()).slice(0, 14);
    if (upd.length) rows.push(row('upd', t('Recently updated'), upd.map((a) => poster(a, 'u:' + a.id, tf('Updated {when}', { when: ago(Date.parse(a.released) / 1000) }))).join('')));
    rows.push(row('new', t('Just added'), oneEach(pool().sort(byNew), true).slice(0, 14).map((a) => poster(a, 'n:' + a.id)).join('')));
    if (d.kind !== 'top') rows.push(row('top', t('Most played'), oneEach(pool().sort(byDl)).slice(0, 14).map((a) => poster(a, 't:' + a.id)).join('')));
    // Covers: every game in the dock fits across, smaller when there are more of them. Wide
    // pictures: as many as fit, and the row slides.
    const wide = (S.settings.ui || {}).tv_dock === 'wide';
    const n = Math.max(1, list.length), avail = (TV.w || 1920) - SX * 2, gap = 22;
    const w = wide ? Math.max(300, Math.min(400, Math.floor((avail - gap * (Math.min(n, 4) - 1)) / Math.min(n, 4)))) : Math.max(168, Math.min(224, Math.floor((avail - gap * (n - 1)) / n)));
    for (const a of list) preload(heroOf(a));
    const dock = list.map((a) => {
      const st = status(a), j = S.jobs[a.id];
      const badge = st === 'running' ? `<span class="tv-badge">${esc(t('Playing'))}</span>` : st === 'update' ? `<span class="tv-badge">${esc(t('Update'))}</span>` : '';
      const pic = wide ? (a.hero || a.cover ? heroOf(a) : '') : a.cover ? media(a.cover) : '';
      return `<button type="button" class="tv-dk${wide ? ' wide' : ''}${TV.sort && TV.sort.id === a.id ? ' moving' : ''}" data-f data-key="c:${esc(a.id)}" data-hero="${esc(a.id)}" data-dock="${esc(a.id)}" aria-label="${esc(a.name)}"><span class="ph">${esc(a.name)}</span>${pic ? `<img src="${esc(pic)}" alt="" decoding="async">` : ''}${wide ? `<span class="cap">${esc(a.name)}</span>` : ''}${badge}${j ? `<span class="tv-pbar"><i data-prog-bar="${esc(a.id)}" style="width:${pct(j)}%"></i></span>` : ''}${TV.sort && TV.sort.id === a.id ? `<span class="tv-mv">${svg('chevL', 26)}${svg('chevR', 26)}</span>` : ''}</button>`;
    }).join('');
    const label = d.kind === 'pinned' ? t('Pinned') : d.kind === 'mine' ? t('Continue playing') : d.kind === 'banner' ? t('Featured') : t('Most played recomps');
    const hintList = TV.sort ? [['dpad', t('Move')], ['A', t('Done')], ['B', t('Cancel')]] : [['A', t('Select')], ['X', t('Options')], ['Y', t('Game page')], ['LB+RB', t('Sections')]];
    return `${backdrop(sel, 'home')}${bar()}
      ${region(`<div class="tv-home1"><div class="tv-hero" id="tv-hero">${sel ? heroText(sel, S.library[sel.id], true) : `<h1 class="tv-h1">${esc(t('Welcome'))}</h1>`}</div>
        <section class="tv-dock${TV.sort ? ' sorting' : ''}" style="--dw:${w}px;--dg:${gap}px"><h3>${esc(label)}</h3><div class="tv-rowview"><div class="tv-row tv-dockrow" data-row="dock">${dock}</div></div></section>
        ${rows.length ? `<div class="tv-down" aria-hidden="true">${svg('chevD', 34)}</div>` : ''}</div>${rows.join('')}`, 'home')}
      ${hints(hintList)}`;
  }

  // What A does for a game, as a sheet item: play, carry on, update, set up or install.
  function mainItem(a) {
    const st = status(a), id = a.id;
    switch (st) {
      case 'running': return { label: t('Back to the game'), icon: 'play', run: () => invoke('resume_game').catch(fail) };
      case 'busy': return { label: t('Cancel the download'), sub: progLabel(S.jobs[id])[0], icon: 'stop', run: () => invoke('cancel', { id }) };
      case 'update': return { label: t('Update now'), sub: S.updates[id] ? tf('{v} is out', { v: S.updates[id] }) : '', icon: 'refresh', run: () => startInstall(id, S.settings.library_dir).then(() => draw(true)) };
      case 'installed': {
        const su = S.setup[id];
        if (needsSetup(a) && !setupDone(id)) return { label: t('Set Up'), sub: t('Setup Required'), icon: 'wrench', run: () => tvSetupReq(id) };
        if (S.easy.has(id) && su && su !== 'loading' && su.needs && !su.copy) return { label: t('Set up'), sub: t('Pick your copy of the game'), icon: 'disc', run: () => tvSetup(id) };
        return { label: isServer(byId(id) || { id }) ? t('Start Server') : t('Play'), icon: 'play', run: () => playGame(id) };
      }
      case 'soon': case 'nobuild': case 'settingup': return null;
      default: return isSaved(id) ? { label: t('Install'), sub: a.size || '', icon: 'down', run: () => startInstall(id, S.settings.library_dir).then(() => draw(true)) } : { label: t('Add to Library'), sub: t('Install it whenever you like'), icon: 'plus', run: () => addToLibrary(id).then(() => draw(true)) };
    }
  }
  // The main button's words, for Home's hero (where the button is the A key, not a target).
  function mainWords(a) {
    const st = status(a);
    if (st === 'busy') return progLabel(S.jobs[a.id])[0];
    if (st === 'soon') return t('Coming soon');
    if (st === 'nobuild') return tf('No {os} version yet', { os: OS_NAME[S.os] });
    if (st === 'settingup') return t('Its setup is open');
    const m = mainItem(a);
    return m ? m.label : t('Game page');
  }
  function heroText(a, g, home) {
    const kick = S.running.has(a.id) ? t('Playing now') : S.updates[a.id] ? t('Update ready') : g ? (g.last_played ? tf('Played {when}', { when: ago(g.last_played) }) : t('In your library')) : (window.bannerInfo && bannerInfo(a.id).tag) || (a.featured ? t('Featured') : t('Most played'));
    const meta = [a.game ? '' : originName(a.origin), g ? (g.played_secs ? tf('{time} played', { time: fmtTime(g.played_secs) }) : '') : a.downloads ? tf('{n} downloads', { n: fmtN(a.downloads) }) : ''].filter(Boolean).join(' · ');
    const acts = home
      ? `<span class="tv-btn main tv-fake" data-hact="main" data-id="${esc(a.id)}">${glyph('a', 'tv-keyg')}<span>${esc(mainWords(a))}</span></span><span class="tv-btn tv-fake" data-hact="page" data-id="${esc(a.id)}">${glyph('y', 'tv-keyg')}<span>${esc(t('Game page'))}</span></span>`
      : `${mainButton(a, 'hero-main')}${btn('hero-page', 'page', esc(t('Game page')), { id: a.id, g: 'y' })}`;
    // On Home the time played joins the line over the title, to leave the logo room.
    return `<span class="tv-kick">${esc(home && meta ? `${kick} · ${g && g.played_secs ? fmtTime(g.played_secs) : meta}` : kick)}</span>${titleOf(a)}${a.game ? `<p class="tv-based">${DISC_S.replace(/14/g, '24')}<span>${htmlIn(t('Based on {game}'), { game: `<b>${esc(a.game)}</b>` })}${a.origin ? ` · ${esc(originName(a.origin))}` : ''}</span></p>` : ''}${meta && !home ? `<p class="tv-meta">${esc(meta)}</p>` : ''}
      <div class="tv-acts">${acts}</div>`;
  }

  // Big grids come in a few rows at a time: eight rows at first, and more as you get near the end,
  // so a page with hundreds of games opens at once and moving around it stays quick.
  const LAZY = new Map();
  let lazyN = 0;
  function grid(list, key, opts = {}) {
    const w = TV.w || stageSize()[0];
    const cols = evenCols(list.length, w - SX * 2, opts.min || 210, opts.gap || 30);
    TV.gridShown = TV.gridShown || {};
    const sig = `${TV.view}:${key}:${list.length}:${list[0] ? list[0].id : ''}`;
    const want = Math.max(cols * 8, TV.gridShown[sig] || 0);
    const first = list.length <= cols * 10 ? list.length : Math.min(list.length, Math.ceil(want / cols) * cols);
    let lazy = '';
    if (first < list.length) {
      const gid = 'z' + ++lazyN;
      LAZY.set(gid, { list, key, opts, shown: first, cols, sig });
      lazy = ` data-lazy="${gid}"`;
    }
    return `<div class="tv-grid ${opts.cls || ''}"${lazy} style="grid-template-columns:repeat(${cols},minmax(0,1fr))">${list.slice(0, first).map((a, i) => (opts.item ? opts.item(a, i) : poster(a, key + ':' + a.id))).join('')}</div>`;
  }
  // The next rows of a grid, once the one you're on is within three rows of the end of what's there.
  function growGrid(el) {
    const g = el && el.closest('.tv-grid[data-lazy]');
    if (!g) return;
    const L = LAZY.get(g.dataset.lazy);
    if (!L) return;
    const i = [...g.children].findIndex((c) => c.contains(el));
    if (i < L.shown - L.cols * 3) return;
    const from = L.shown, next = L.list.slice(from, from + L.cols * 6);
    L.shown += next.length;
    TV.gridShown[L.sig] = L.shown;
    g.insertAdjacentHTML('beforeend', next.map((a, j) => (L.opts.item ? L.opts.item(a, from + j) : poster(a, L.key + ':' + a.id))).join(''));
    if (L.shown >= L.list.length) { LAZY.delete(g.dataset.lazy); g.removeAttribute('data-lazy'); }
  }

  // The Library laid out the way the player picked (Settings, Library View): a grid of covers, or
  // Cover Flow (one row of cases, the focused one up front and the rest turned away).
  function libView(list) {
    const v = (S.settings.ui || {}).tv_view;
    if (v === 'flow') {
      // Laid out in the page itself, around the game you were on, so coming back doesn't slide in.
      // Each game is a block of tinted glass with real thickness (solid sides running back from its
      // face), the cover set into it, and its reflection on the floor.
      const at = Math.max(0, list.findIndex((a) => a.id === TV.flowId));
      FLOW.row = null;
      const box = (a, k) => {
        const g = flowGeom(k - at), pic = a.cover ? esc(media(a.cover)) : '';
        const badge = S.running.has(a.id) ? `<span class="tv-badge">${esc(t('Playing'))}</span>` : S.updates[a.id] ? `<span class="tv-badge">${esc(t('Update'))}</span>` : sreqBadge(a);
        const sl = '<span class="wl l"></span><span class="wl r"></span><span class="wl t"></span><span class="wl b"></span><span class="wl k"></span>';
        // A div, not a button: a button's own inner box flattens everything in it, and the block needs real depth.
        return `<div role="button" tabindex="-1" class="tv-fcase" style="${g.css}" data-f data-key="L:${esc(a.id)}" data-hero="${esc(a.id)}" data-go-game="${esc(a.id)}" aria-label="${esc(a.name)}"><span class="bx">${sl}<span class="plate"><span class="front"><span class="ph">${esc(a.name)}</span>${pic ? `<img src="${pic}" alt="" decoding="async">` : ''}<span class="shade"></span>${badge}</span></span>${pic && !lighter() ? `<span class="refl"><img src="${pic}" alt="" decoding="async"></span>` : ''}</span></div>`;
      };
      return `<div class="tv-flow"><div class="tv-flow-floor" aria-hidden="true"></div><div class="tv-flow-row tv-row" data-row="flow">${list.map(box).join('')}</div>
        <div class="tv-flow-cap" id="tv-flowcap"></div></div>`;
    }
    return grid(list, 'L');
  }
  // Cover Flow, drawn every frame from one number: where along the row the view is. It glides to
  // the game you move to, however fast you go, so nothing jumps; each case's place comes from how
  // far it is from that point.
  const FLOW = { pos: 0, target: 0, raf: 0, row: null, items: [], last: 0 };
  const FLOW_P = 1500;
  // Where a block stands, d places from the one in front (as Porpoise lays them out): the first on
  // each side close by, a little smaller and turned a touch toward the middle, the rest stepping back
  // smaller still, and fading out before they reach the edge of the screen.
  // Laid out like a shelf: each cover a little smaller and turned a touch toward the middle,
  // with a small even gap between them, the one in focus up front and biggest.
  const FLOW_W = 392, FLOW_GAP = 6, FLOW_YAW = 15;
  const flowSc = a => a <= 1 ? 1 - 0.17 * a : Math.max(0.6, 0.83 - 0.06 * (a - 1));
  const flowHw = a => FLOW_W / 2 * flowSc(a) * Math.cos(Math.min(a, 1) * FLOW_YAW * Math.PI / 180);
  const FLOW_X = [0];
  for (let n = 1; n <= 9; n++) FLOW_X[n] = FLOW_X[n - 1] + flowHw(n - 1) + FLOW_GAP + flowHw(n);
  function flowGeom(d) {
    const a = Math.abs(d), sg = d < 0 ? -1 : 1;
    const n = Math.min(8, Math.floor(a)), f = Math.min(1, a - n);
    const x = sg * (FLOW_X[n] + (FLOW_X[n + 1] - FLOW_X[n]) * f);
    const sc = flowSc(a), yaw = sg * Math.min(a, 1) * FLOW_YAW;
    // Drawn smaller by standing further back, so the room's perspective shows each block's
    // thickness on the side that faces the middle.
    const z = FLOW_P * (1 / sc - 1);
    const al = Math.max(0, Math.min(1, 1 - (a - 5)));
    const dim = Math.min(0.28, a * 0.12);
    const hide = a > 6.2 || al <= 0;
    return { css: `transform:translate3d(${(x / sc).toFixed(1)}px,0,${(-z).toFixed(1)}px) rotateY(${yaw.toFixed(2)}deg);--al:${al.toFixed(3)};--dim:${dim.toFixed(3)}${hide ? ';visibility:hidden' : ''}`, x, sc, yaw, z, al, dim, hide };
  }
  function flowPaint() {
    const row = FLOW.row;
    if (!row || !row.isConnected) return false;
    const lo = Math.floor(FLOW.pos) - 7, hi = Math.ceil(FLOW.pos) + 7;
    FLOW.items.forEach((c, k) => {
      if (k < lo || k > hi) { if (c.style.visibility !== 'hidden') c.style.visibility = 'hidden'; return; }
      const g = flowGeom(k - FLOW.pos);
      c.style.visibility = g.hide ? 'hidden' : '';
      if (g.hide) return;
      c.style.transform = `translate3d(${(g.x / g.sc).toFixed(1)}px,0,${(-g.z).toFixed(1)}px) rotateY(${g.yaw.toFixed(2)}deg)`;
      // Faded one part at a time: fading the whole block would flatten it.
      c.style.setProperty('--al', g.al.toFixed(3));
      c.style.setProperty('--dim', g.dim.toFixed(3));
    });
    return true;
  }
  function flowTick(now) {
    const dt = Math.min(0.05, FLOW.last ? (now - FLOW.last) / 1000 : 1 / 60);
    FLOW.last = now;
    const diff = FLOW.target - FLOW.pos;
    if (Math.abs(diff) < 0.002) { FLOW.pos = FLOW.target; flowPaint(); FLOW.raf = 0; FLOW.last = 0; return; }
    FLOW.pos += diff * (1 - Math.exp(-dt * 10));
    if (!flowPaint()) { FLOW.raf = 0; FLOW.last = 0; return; }
    FLOW.raf = requestAnimationFrame(flowTick);
  }
  function flowLayout(el, instant) {
    const row = el.closest('.tv-flow-row');
    if (!row) return;
    if (FLOW.row !== row) { FLOW.row = row; FLOW.items = $$('.tv-fcase', row); instant = true; }
    const i = FLOW.items.indexOf(el);
    FLOW.target = i;
    if (instant || lighter()) { FLOW.pos = i; flowPaint(); }
    else if (!FLOW.raf) { FLOW.last = 0; FLOW.raf = requestAnimationFrame(flowTick); }
    TV.flowId = el.dataset.hero;
    // The art for the games either side is fetched ahead, so the backdrop is ready when you get there.
    for (let k = Math.max(0, i - 3); k <= Math.min(FLOW.items.length - 1, i + 3); k++) preload(heroOf(libAppOf(FLOW.items[k].dataset.hero)));
    tiltStart();
    const a = libAppOf(el.dataset.hero);
    const g = S.library[el.dataset.hero];
    const cap = $('#tv-flowcap', root);
    if (cap) { cap.innerHTML = `<b>${esc(a.game || a.name)}</b><span>${esc(g ? timeLine(g) : '')}</span>`; cap.classList.remove('swap'); void cap.offsetWidth; cap.classList.add('swap'); }
    setTone(el.dataset.hero);
  }

  // Pictures fetched ahead of time (kept while they're likely to be needed).
  const PRE = new Map();
  function preload(src) {
    if (!src || PRE.has(src)) return;
    const im = new Image();
    im.decoding = 'async';
    im.src = src;
    PRE.set(src, im);
    if (PRE.size > 60) PRE.delete(PRE.keys().next().value);
  }
  // The box you're on tilts with the right stick (or the mouse over it), like turning it in your hands.
  const TILT = { raf: 0, x: 0, y: 0, mx: 0, my: 0, mouseAt: 0, el: null };
  function tiltStart() {
    if (TILT.raf || lighter()) return;
    TILT.raf = requestAnimationFrame(tiltTick);
  }
  // At rest the box you're on faces straight ahead, front and centre; the right stick turns it.
  const REST = { y: 0, x: 0 };
  function tiltTick() {
    TILT.raf = 0;
    const f = FLOW.row && FLOW.row.isConnected && $('.tv-fcase.f .bx', FLOW.row);
    if (TILT.el && TILT.el !== f) TILT.el.style.transform = '';
    TILT.el = f;
    if (!f || !TV.on) { TILT.x = TILT.y = 0; return; }
    let sx = 0, sy = 0;
    if (performance.now() - TILT.mouseAt < 1500) { sx = TILT.mx; sy = TILT.my; }
    else if (navigator.getGamepads) {
      for (const gp of navigator.getGamepads()) {
        if (!gp || gp.axes.length < 4) continue;
        const x = gp.axes[2] || 0, y = gp.axes[3] || 0;
        if (Math.hypot(x, y) > Math.hypot(sx, sy)) { sx = x; sy = y; }
      }
      if (Math.hypot(sx, sy) < 0.15) sx = sy = 0;
    }
    const tx = sx * 22, ty = -sy * 14;
    TILT.x += (tx - TILT.x) * 0.16;
    TILT.y += (ty - TILT.y) * 0.16;
    f.style.transform = Math.abs(TILT.x) + Math.abs(TILT.y) < 0.05 ? '' : `rotateY(${(REST.y + TILT.x).toFixed(2)}deg) rotateX(${(REST.x + TILT.y).toFixed(2)}deg)`;
    // Still and centred: the loop rests, and the next stick or mouse movement wakes it.
    if (Math.abs(TILT.x) + Math.abs(TILT.y) < 0.05 && !sx && !sy) { TILT.idle = setTimeout(tiltStart, 150); return; }

    TILT.raf = requestAnimationFrame(tiltTick);
  }
  root.addEventListener('pointermove', (e) => {
    const f = e.target.closest && e.target.closest('.tv-fcase.f');
    if (!f) return;
    const r = f.getBoundingClientRect();
    TILT.mx = Math.max(-1, Math.min(1, ((e.clientX - r.left) / r.width - 0.5) * 2));
    TILT.my = Math.max(-1, Math.min(1, ((e.clientY - r.top) / r.height - 0.5) * 2));
    TILT.mouseAt = performance.now();
    tiltStart();
  });

  // Opening a game: the camera flies into the cover you picked and comes out on the game's page.
  // The page you were on grows toward you around that cover and fades, while the game's page settles
  // in from just behind the screen. Going back runs the other way, out to the cover you came from.
  // Only transforms and opacity move, so it stays smooth on handhelds too.
  const FLY = { D: 280, IN: 'cubic-bezier(.45,0,.6,.4)', OUT: 'cubic-bezier(.2,.75,.15,1)' };
  const still = () => lighter() || matchMedia('(prefers-reduced-motion: reduce)').matches;
  const calm = () => matchMedia('(prefers-reduced-motion: reduce)').matches;
  // The cover's own rectangle on screen (a Cover Flow box: its front).
  const faceOf = (el) => (el.querySelector('.front') || el).getBoundingClientRect();
  // A point on screen, in the page's own units (it's drawn at 1920 wide and scaled to the screen).
  function stagePt(st, x, y) { const s = st.getBoundingClientRect(), k = s.width / st.offsetWidth || 1; return { x: (x - s.left) / k, y: (y - s.top) / k }; }
  // What moves with the camera (the backdrop and the page itself) and what only fades (bar and hints).
  const movers = (st) => $$(':scope > .tv-full, :scope > .tv-region', st);
  const fixers = (st) => $$(':scope > .tv-bar, :scope > .tv-hints', st);
  // For each part that moves: point p brought to the middle of the screen, s times bigger.
  const camera = (st, p, s) => movers(st).map((m) => ({ m, origin: `${(p.x - m.offsetLeft).toFixed(1)}px ${(p.y - m.offsetTop).toFixed(1)}px`, to: `translate(${(st.offsetWidth / 2 - p.x).toFixed(1)}px,${(st.offsetHeight / 2 - p.y).toFixed(1)}px) scale(${s})` }));
  // The first things on a page, which come up one after another as it arrives.
  function bitsOf(st) {
    const sc = $('.tv-scroll', st);
    let kids = sc ? [...sc.children] : [];
    if (kids.length === 1) kids = [...kids[0].children];
    return kids.slice(0, 6);
  }
  // The camera has stopped: anything that wanted to redraw meanwhile does it now.
  function landed() {
    clearTimeout(TV.flyGuard);
    TV.flying = false;
    if (TV.later) { TV.later = false; draw(true); }
  }
  // Where the picture of the cover is: moved and grown, and turned like the block it stands for
  // (the block you're on rests turned a touch), or flat.
  const cardPose = (x, y, k, turned) => `translate(${x.toFixed(1)}px,${y.toFixed(1)}px) scale(${k.toFixed(3)}) perspective(1500px) rotateY(${turned ? REST.y : 0}deg) rotateX(${turned ? REST.x : 0}deg)`;
  // A flat picture of a Cover Flow block's cover, over everything, exactly where the cover is.
  function shelfCard(el, r) {
    if (!el.matches('.tv-fcase')) return null;
    const img = $('.front img', el);
    if (!img || !img.complete || !img.naturalWidth) return null;
    const c = document.createElement('div');
    c.className = 'tv-thru-card';
    // Its true size (the box on screen is a touch narrower, as the block stands turned), centred on it.
    const f = $('.front', el), st = el.closest('.tv-stage'), k = st ? st.getBoundingClientRect().width / st.offsetWidth : 1;
    const w = f ? f.offsetWidth * k : r.width, h = f ? f.offsetHeight * k : r.height;
    Object.assign(c.style, { left: r.left + r.width / 2 - w / 2 + 'px', top: r.top + r.height / 2 - h / 2 + 'px', width: w + 'px', height: h + 'px' });
    c.innerHTML = `<img src="${esc(img.getAttribute('src'))}" alt="">`;
    root.append(c);
    return c;
  }
  function fly(el, id) {
    if (TV.flying) return;
    const old = stage(), r = faceOf(el);
    if (calm() || !old || !r.width) return go('game', id);
    const a = libAppOf(id), D = FLY.D;
    sfx('open');
    TV.flying = true;
    // Whatever happens on the way, the controls never stay locked.
    clearTimeout(TV.flyGuard); TV.flyGuard = setTimeout(() => { if (TV.flying) landed(); }, D * 3);
    setTone(id);
    const p = stagePt(old, r.left + r.width / 2, r.top + r.height / 2);
    // The art the page opens on is decoded while the camera moves, so it's there when the page shows.
    const src = heroOf(a);
    preload(src);
    const im = src && PRE.get(src);
    const ready = im && im.decode ? im.decode().catch(() => {}) : Promise.resolve();
    old.style.pointerEvents = 'none';
    const card = shelfCard(el, r);
    if (card) {
      // Cover Flow: the shelf's glass blocks stay as they are and only fade, a little closer; a flat
      // picture of the cover is what the camera flies through. (Blowing the 3D shelf itself up to
      // three times its size is too much for some graphics cards, and it showed as flicker.)
      for (const c of camera(old, p, 1.08)) {
        c.m.style.transformOrigin = c.origin;
        c.m.animate([{ transform: 'none', opacity: 1 }, { transform: `scale(1.08)`, opacity: 0 }], { duration: D * 0.7, easing: 'ease-in', fill: 'forwards' });
      }
      el.style.visibility = 'hidden';
      const k = Math.max(innerWidth / r.width, innerHeight / r.height) * 1.15;
      card.animate([{ transform: cardPose(0, 0, 1, true) }, { transform: cardPose(innerWidth / 2 - r.left - r.width / 2, innerHeight / 2 - r.top - r.height / 2, k, false) }], { duration: D, easing: FLY.IN, fill: 'forwards' });
      card.animate([{ opacity: 1 }, { opacity: 1, offset: 0.4 }, { opacity: 0 }], { duration: D, easing: 'ease-in', fill: 'forwards' }).onfinish = () => card.remove();
    } else {
      for (const c of camera(old, p, 3.2)) {
        c.m.style.transformOrigin = c.origin;
        c.m.animate([{ transform: 'none' }, { transform: c.to }], { duration: D, easing: FLY.IN, fill: 'forwards' });
        // It's gone a little before the camera gets there, so the two pages are never both clear at once.
        c.m.animate([{ opacity: 1 }, { opacity: 1, offset: 0.3 }, { opacity: 0 }], { duration: D * 0.8, easing: 'ease-in', fill: 'forwards' });
      }
    }
    for (const f of fixers(old)) f.animate([{ opacity: 1 }, { opacity: 0 }], { duration: D * 0.45, easing: 'ease-out', fill: 'forwards' });
    const t0 = performance.now();
    Promise.race([ready, wait(D * 0.6)]).then(() => wait(Math.max(0, D * 0.35 - (performance.now() - t0)))).then(() => {
      go('game', id, { enter: 'thru', quiet: true, fly: { key: el.dataset.key } });
      const st = stage(), ms = Math.round(D * 0.82);
      if (!st) return landed();
      st.animate([{ opacity: 0 }, { opacity: 1 }], { duration: ms, easing: FLY.OUT });
      // The game's page settles in from just behind the screen: its art a touch slower than the page on it.
      for (const m of movers(st)) { m.animate([{ transform: m.matches('.tv-full') ? 'scale(.93)' : 'scale(.86)' }, { transform: 'none' }], { duration: ms, easing: FLY.OUT }); }
      bitsOf(st).forEach((b, i) => b.animate([{ opacity: 0, transform: 'translateY(18px)' }, { opacity: 1, transform: 'none' }], { duration: Math.round(D * 0.7), delay: Math.round(D * 0.2 + i * D * 0.06), easing: FLY.OUT, fill: 'backwards' }));
      // Gone once the camera has passed through it (it's fully faded by then).
      setTimeout(() => old.remove(), D + 30);
      setTimeout(() => {
        st.classList.remove('tvx-thru');
        landed();
      }, ms + 40);
    });
  }
  function flyBack(f) {
    const st = stage(), old = $('.tv-stage.leaving', root);
    const done = () => {
      if (old) old.remove();
      if (st) { st.classList.remove('tvx-thruback'); for (const m of movers(st)) m.style.transformOrigin = ''; }
      landed();
    };
    const el = f && st && $(`[data-key="${CSS.escape(f.key)}"]`, st);
    const r = el && faceOf(el);
    if (!r || !r.width || calm()) {
      // Nothing to fly back to: the game's page simply fades away.
      if (old) { old.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 200, easing: 'ease-out', fill: 'forwards' }).onfinish = done; return; }
      return done();
    }
    TV.flying = true;
    const p = stagePt(st, r.left + r.width / 2, r.top + r.height / 2), D = Math.round(FLY.D * 0.85);
    clearTimeout(TV.flyGuard); TV.flyGuard = setTimeout(() => { if (TV.flying) done(); }, D * 3);
    if (old) {
      for (const m of movers(old)) m.animate([{ opacity: 1, transform: 'none' }, { opacity: 0, transform: 'scale(.9)' }], { duration: D * 0.45, easing: FLY.IN, fill: 'forwards' });
      for (const x of fixers(old)) x.animate([{ opacity: 1 }, { opacity: 0 }], { duration: D * 0.45, fill: 'forwards' });
    }
    const card = shelfCard(el, r);
    if (card) {
      // Back to Cover Flow: the picture of the cover flies back onto the shelf, which fades in
      // around it, and hands over to the block itself.
      el.style.visibility = 'hidden';
      const k = Math.max(innerWidth / r.width, innerHeight / r.height) * 1.15;
      card.animate([{ transform: cardPose(innerWidth / 2 - r.left - r.width / 2, innerHeight / 2 - r.top - r.height / 2, k, false), opacity: 0 }, { opacity: 1, offset: 0.35 }, { transform: cardPose(0, 0, 1, true), opacity: 1 }], { duration: D, delay: D * 0.1, easing: FLY.OUT, fill: 'both' });
      for (const m of movers(st)) {
        m.style.transformOrigin = `${(p.x - m.offsetLeft).toFixed(1)}px ${(p.y - m.offsetTop).toFixed(1)}px`;
        m.animate([{ transform: 'scale(1.08)', opacity: 0 }, { transform: 'none', opacity: 1 }], { duration: D, delay: D * 0.2, easing: FLY.OUT, fill: 'backwards' });
      }
      setTimeout(() => {
        el.style.visibility = '';
        card.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 110, easing: 'ease-out', fill: 'forwards' }).onfinish = () => card.remove();
      }, Math.round(D * 1.1) + 10);
    } else {
      for (const c of camera(st, p, 2.2)) {
        c.m.style.transformOrigin = c.origin;
        c.m.animate([{ transform: c.to, opacity: 0 }, { transform: 'none', opacity: 1 }], { duration: D, delay: D * 0.2, easing: FLY.OUT, fill: 'backwards' });
      }
    }
    for (const x of fixers(st)) x.animate([{ opacity: 0 }, { opacity: 1 }], { duration: D * 0.6, delay: D * 0.2, fill: 'backwards' });
    setTimeout(done, Math.round(D * 1.2) + 40);
  }
  function pageLibrary() {
    // Installed games first, then the ones added to the Library but not installed yet.
    const list = installed().map((x) => x.a).concat(Object.keys(saved()).filter((id) => !S.library[id] && byId(id)).map((id) => byId(id)));
    const flow = (S.settings.ui || {}).tv_view === 'flow';
    return `${backdrop(null, flow ? 'flowbg' : '')}${bar()}
      ${region(list.length ? libView(list) : `<p class="tv-empty">${esc(t('Install a recomp from Discover and it shows up here'))}</p>`, flow ? 'flowreg' : 'libreg')}
      ${hints([['A', t('Open')], ['X', t('Options')], ['B', t('Back')], ['LB+RB', t('Sections')]])}`;
  }

  // Discover's Sort & Filter: the order, a console, and a way back to everything.
  function discoverRedraw() { TV.swap = true; TV.focusKey = TV.focusKey || 'g:@filter'; TV.gridShown = {}; draw(true); setScroll(0, true); }
  function tvFilter() {
    const apps = pool(), org = TV.dorigin || '', srt = TV.dsort || 'popular';
    const SORTS = [['popular', t('Popular')], ['new', t('Newest')], ['updated', t('Recently updated')], ['stars', t('Most stars')], ['az', t('A to Z')]];
    const pickShow = () => sheet(t('Show'), SHOWS.map(([k, n]) => ({ label: t(n), icon: (TV.dshow || '') === k ? 'check' : '', run: () => { TV.dshow = k; TV.focusKey = 'g:@filter'; discoverRedraw(); } })));
    const origins = ((S.cat && S.cat.origins) || []).map((o) => [o, apps.filter((a) => a.origin === o.id).length]).filter(([, n]) => n);
    const pickSort = () => sheet(t('Sort'), SORTS.map(([k, n]) => ({ label: n, icon: k === srt ? 'check' : '', run: () => { TV.dsort = k; TV.focusKey = 'g:@filter'; discoverRedraw(); } })));
    const pickConsole = () => sheet(t('Console'), [{ label: t('Any'), icon: org ? '' : 'check', run: () => { TV.dorigin = ''; TV.focusKey = 'g:@filter'; discoverRedraw(); } }].concat(origins.map(([o, n]) => ({ label: o.name, sub: tn(n, '1 recomp', '{n} recomps'), icon: o.id === org ? 'check' : '', run: () => { TV.dorigin = o.id; TV.focusKey = 'g:@filter'; discoverRedraw(); } }))));
    const tagN = {};
    for (const a of apps) for (const tg of tagsOf(a)) tagN[tg] = (tagN[tg] || 0) + 1;
    const pickTag = () => sheet(t('Tag'), [{ label: t('Any'), icon: TV.dtag ? '' : 'check', run: () => { TV.dtag = ''; TV.focusKey = 'g:@filter'; discoverRedraw(); } }].concat(Object.keys(tagN).sort((x, y) => tagN[y] - tagN[x]).map((tg) => ({ label: tg, sub: tn(tagN[tg], '1 recomp', '{n} recomps'), icon: tg === TV.dtag ? 'check' : '', run: () => { TV.dtag = tg; TV.focusKey = 'g:@filter'; discoverRedraw(); } }))));
    sheet(t('Sort & Filter'), [
      { label: t('Sort'), sub: (SORTS.find(([k]) => k === srt) || SORTS[0])[1], icon: 'layers', run: pickSort },
      { label: t('Console'), sub: org ? originName(org) : t('Any'), icon: 'pad', run: pickConsole },
      { label: t('Tag'), sub: TV.dtag || t('Any'), icon: 'pin', run: pickTag },
      { label: t('Show'), sub: t((SHOWS.find(([k]) => k === (TV.dshow || '')) || SHOWS[0])[1]), icon: 'layers', run: pickShow },
      { label: t('Plays great'), sub: TV.dgreat ? t('On') : t('Off'), icon: 'check', run: () => { TV.dgreat = !TV.dgreat; TV.focusKey = 'g:@filter'; discoverRedraw(); } },
      { label: t('Search'), sub: TV.dq ? TV.dq : t('Find a game by name'), icon: 'search', run: tvSearch },
      { label: t('Clear All'), sub: t('Every recomp, most popular first'), icon: 'refresh', gap: true, run: () => { TV.dq = ''; TV.dorigin = ''; TV.dtag = ''; TV.dshow = ''; TV.dgreat = false; TV.dsort = 'popular'; TV.genre = ''; TV.focusKey = 'g:@filter'; discoverRedraw(); } },
    ]);
  }
  // Search with the controller: an on-screen keyboard (a keyboard types straight in too). The games
  // narrow as you type.
  const KEYS = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789'.split('');
  function tvSearch() {
    TV.dq = TV.dq || '';
    let later = 0;
    const show = () => { const f = $('#tv-kbq', root); if (f) f.textContent = TV.dq || ''; const ph = $('#tv-kbph', root); if (ph) ph.hidden = !!TV.dq; };
    const changed = () => { show(); clearTimeout(later); later = setTimeout(() => { TV.gridShown = {}; TV.focusKey = 'g:@search'; draw(true); setScroll(0, true); }, 220); };
    const type = (c) => { if (TV.dq.length < 40) { TV.dq += c; changed(); } };
    const items = KEYS.map((k) => ({ label: k, cls: 'key', run: () => type(k.toLowerCase()) }))
      .concat([{ label: t('Space'), cls: 'key wide', run: () => type(' ') }, { label: t('Delete'), cls: 'key wide', icon: 'erase', run: () => { TV.dq = TV.dq.slice(0, -1); changed(); } }, { label: t('Clear'), cls: 'key wide', run: () => { TV.dq = ''; changed(); } }, { label: t('Done'), cls: 'key wide done', icon: 'check', run: () => { TV.sheet.keep = false; closeSheet(); } }]);
    const head = `<h3>${esc(t('Search'))}</h3><div class="tv-kb-field">${svg('search', 26)}<span id="tv-kbq"></span><span id="tv-kbph" class="ph">${esc(t('Type a game\'s name'))}</span><i class="caret"></i></div><div class="tv-kb-hints"><span>${glyph('y')}${esc(t('Space'))}</span><span>${glyph('x')}${esc(t('Delete'))}</span></div>`;
    sheet(t('Search'), items, 'kb', head);
    TV.sheet.keep = true;
    TV.sheet.kb = { type, del: () => { TV.dq = TV.dq.slice(0, -1); changed(); } };
    // Like a console's keyboard: Y types a space, X deletes.
    TV.sheet.keys = (b) => { if (b === 'y') { type(' '); sfx('move'); return true; } if (b === 'x') { TV.sheet.kb.del(); sfx('move'); return true; } return false; };
    TV.sheet.onClose = () => { clearTimeout(later); TV.gridShown = {}; TV.focusKey = 'g:@search'; draw(true); setScroll(0, true); };
    show();
  }
  // Achievements, coming soon: what it'll be, Learn More, and Sign In when you aren't.
  function pageAchTV() {
    const signed = !!(S.account && S.account.signedIn);
    const cards = achCards().map(([h, d, ic]) => `<div class="tv-ach-card"><span class="lock" aria-hidden="true">${LOCK.replace(/16/g, '22')}</span><span class="ic" aria-hidden="true"><svg width="30" height="30" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">${ic}</svg></span><b>${esc(h)}</b><p>${esc(d)}</p></div>`).join('');
    // Signing in is the one thing to do here from the couch; the website is for the desktop.
    const acts = signed ? '' : btn('ach-in', 'signin', esc(t('Sign in')), { cls: 'main', g: 'a' });
    return `${backdrop(null)}${bar()}
      ${region(`<div class="tv-ach">${pageHead(t('Achievements'), t('Earn achievements in your recomps, track them on your profile and climb the leaderboards. We\'re preparing the system. More info soon.'))}<span class="tv-ach-pill">${esc(t('Coming soon'))}</span>${signed ? `<p class="tv-ach-ok">${esc(t('You\'re all set. Your achievements will be tied to your account.'))}</p>` : ''}
        <div class="tv-ach-cards">${cards}</div>
        ${acts ? `<div class="tv-acts">${acts}</div>` : ''}</div>`)}
      ${hints([['A', t('Select')], ['B', t('Back')], ['LB+RB', t('Sections')]])}`;
  }
  function pageDiscover() {
    const g = TV.genre || '';
    const apps = pool();
    const genres = Object.keys(GENRES).filter((x) => apps.some((a) => a.genre === x));
    // "Easy Setup" is a chip of its own, before the genres.
    // Search, a console and the order narrow it further (Sort & Filter).
    const q = (TV.dq || '').trim().toLowerCase(), org = TV.dorigin || '', srt = TV.dsort || 'popular';
    const order = srt === 'new' ? byNew : srt === 'updated' ? byUpdated : srt === 'az' ? (x, y) => x.name.localeCompare(y.name) : srt === 'stars' ? (x, y) => (y.stars || 0) - (x.stars || 0) : byDl;
    const list = oneEach(apps.filter((a) => (g === 'easy' ? S.easy.has(a.id) && !a.soon : g === 'mp' ? isMulti(a) : !g || a.genre === g))
      .filter((a) => !org || a.origin === org)
      .filter((a) => !TV.dtag || tagsOf(a).includes(TV.dtag))
      .filter((a) => showOk(a, TV.dshow) && (!TV.dgreat || playsGreat(a)))
      .filter((a) => !q || `${a.name} ${a.game || ''} ${a.author || ''} ${tagsOf(a).join(' ')}`.toLowerCase().includes(q))
      .sort((x, y) => (x.soon ? 1 : 0) - (y.soon ? 1 : 0) || order(x, y)), srt === 'new');
    const nf = (org ? 1 : 0) + (srt !== 'popular' ? 1 : 0) + (TV.dtag ? 1 : 0) + (TV.dshow ? 1 : 0) + (TV.dgreat ? 1 : 0);
    const tagChip = TV.dtag ? `<button type="button" class="tv-chip tool act" data-f data-key="g:@tag" data-tvact="dtagclear">${esc(t('Tag'))}: ${esc(TV.dtag)} ${svg('stop', 18)}</button>` : '';
    const tools = tagChip + `<button type="button" class="tv-chip tool${q ? ' act' : ''}" data-f data-key="g:@search" data-tvact="dsearch">${svg('search', 20)}${esc(q ? `“${TV.dq.trim()}”` : t('Search'))}</button><button type="button" class="tv-chip tool${nf ? ' act' : ''}" data-f data-key="g:@filter" data-tvact="dfilter">${svg('filter', 20)}${esc(t('Sort & Filter'))}${nf ? `<b class="tv-chip-n">${nf}</b>` : ''}</button>`;
    const chips = `<div class="tv-rowview chips"><div class="tv-row tv-chips" data-row="genres"><span class="tv-chip-pill" aria-hidden="true"></span>${tools}${[['', N_('All')]].concat(apps.some((a) => S.easy.has(a.id)) ? [['easy', N_('Easy Setup')]] : [], apps.some(isMulti) ? [['mp', N_('Multiplayer')]] : [], genres.map((x) => [x, GENRES[x]])).map(([x, n]) => `<button type="button" class="tv-chip${g === x ? ' on' : ''}" data-f data-key="g:${x}" data-genre="${x}">${x === 'easy' ? DISC_S.replace(/14/g, '20') : x === 'mp' ? '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><circle cx="9" cy="8" r="3.2"/><circle cx="17" cy="9" r="2.6"/><path d="M3 19c.6-3.3 3-5 6-5s5.4 1.7 6 5M15.5 14.2c2.7-.3 4.8 1.2 5.5 4.3"/></svg>' : catIcon(x)}${esc(t(n))}</button>`).join('')}</div></div>`;
    return `${backdrop(null)}${bar()}
      ${region(`${pageHead(q ? tf('Results for “{q}”', { q: TV.dq.trim() }) : TV.dtag ? tf('All recomps with {tag}', { tag: TV.dtag }) : org ? originName(org) : g === 'easy' ? t('Easy Setup') : g === 'mp' ? t('Multiplayer') : g ? t(GENRES[g]) : t('Discover'), list.length === 1 ? tf('1 recomp for {os}. You bring your own copy of each game.', { n: 1, os: OS_NAME[S.os] }) : tf('{n} recomps for {os}. You bring your own copy of each game.', { n: list.length, os: OS_NAME[S.os] }))}${chips}
        <div class="tv-gridwrap${TV.swap ? ' swap' : ''}">${list.length ? grid(list, 'D') : `<p class="tv-empty">${esc(q || org || TV.dtag ? t('Nothing matches. Try another search or clear the filters.') : t('Nothing here yet.'))}</p>`}</div>`)}
      ${hints([['A', t('Open')], ['X', t('Options')], ['Y', t('Game page')], ['LB+RB', t('Sections')]])}`;
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
          <button type="button" class="tv-item" data-f data-key="s:size" data-tvact="size" data-seg="1" data-val="${esc(t(size))}"><span class="k">${esc(t('Size'))}<small>${esc(t('How big everything is in Big Picture mode'))}</small></span><span class="tv-seg">${[N_('Auto'), N_('Smaller'), N_('Medium'), N_('Larger')].map((n) => `<span class="${n === size ? 'on' : ''}">${esc(t(n))}</span>`).join('')}</span></button>
          ${(() => { const v = { flow: 'Cover Flow' }[(S.settings.ui || {}).tv_view] || 'Grid'; return `<button type="button" class="tv-item" data-f data-key="s:view" data-tvact="libview" data-seg="1" data-val="${esc(t(v))}"><span class="k">${esc(t('Library View'))}<small>${esc(t('How your games are laid out in the Library'))}</small></span><span class="tv-seg">${[N_('Grid'), N_('Cover Flow')].map((n) => `<span class="${n === v ? 'on' : ''}">${esc(t(n))}</span>`).join('')}</span></button>`; })()}
          ${(() => { const ui = S.settings.ui || {}; const v = ui.theme === 'light' ? 'Light' : ui.oled ? 'OLED' : 'Dark'; return `<button type="button" class="tv-item" data-f data-key="s:theme" data-tvact="theme" data-seg="1" data-val="${esc(v === 'Light' ? tc('Light|theme') : t(v))}"><span class="k">${esc(t('Theme'))}<small>${esc(t('Dark, black for OLED screens, or light.'))}</small></span><span class="tv-seg">${[N_('Dark'), N_('OLED'), N_('Light|theme')].map((n) => `<span class="${n.split('|')[0] === v ? 'on' : ''}">${esc(tc(n))}</span>`).join('')}</span></button>`; })()}
          ${(() => { const v = homePinned() ? 'Pinned Games' : 'Continue Playing'; return `<button type="button" class="tv-item" data-f data-key="s:home" data-tvact="homeview" data-seg="1" data-val="${esc(t(v))}"><span class="k">${esc(t('Home Shows'))}<small>${esc(t('Pin games with X on any game. On Home, press X on a pinned game and pick Move to change its place.'))}</small></span><span class="tv-seg">${[N_('Continue Playing'), N_('Pinned Games')].map((n) => `<span class="${n === v ? 'on' : ''}">${esc(t(n))}</span>`).join('')}</span></button>`; })()}
          ${(() => { const v = { grid: 'Grid', drift: 'Drift', art: 'Game Art' }[bgMode()] || 'Aurora'; return `<button type="button" class="tv-item" data-f data-key="s:bg" data-tvact="bgmode" data-seg="1" data-val="${esc(t(v))}"><span class="k">${esc(t('Backdrop'))}<small>${esc(t('Behind Home and the Library: a moving backdrop in the launcher\'s colors, or the art of the game you\'re on'))}</small></span><span class="tv-seg">${[N_('Aurora'), N_('Grid'), N_('Drift'), N_('Game Art')].map((n) => `<span class="${n === v ? 'on' : ''}">${esc(t(n))}</span>`).join('')}</span></button>`; })()}
          ${(() => { const v = (S.settings.ui || {}).tv_dock === 'wide' ? 'Wide' : 'Covers'; return `<button type="button" class="tv-item" data-f data-key="s:dock" data-tvact="dockview" data-seg="1" data-val="${esc(t(v))}"><span class="k">${esc(t('Home Row'))}<small>${esc(t('Your games on Home as covers, or as wide pictures'))}</small></span><span class="tv-seg">${[N_('Covers'), N_('Wide')].map((n) => `<span class="${n === v ? 'on' : ''}">${esc(t(n))}</span>`).join('')}</span></button>`; })()}
          <button type="button" class="tv-item" data-f data-key="s:lang" data-tvact="lang" data-val="${esc(langName())}"><span class="k">${esc(t('Language'))}<small>${esc(langName())}</small></span><span class="tv-pill on">${esc(t('Change'))}</span></button>
          ${S.appimage ? '' : sw('sounds', t('Sounds'), t('The start-up sound and a soft click as you move around'), S.settings.sounds !== false)}
          ${head(t('Starting up'))}
          ${sw('start_in_tv', t('Start in Big Picture mode'), t('Open full screen, ready for a controller'))}
          ${head(t('Games and updates'))}
          ${sw('notify', t('Notifications'), t('When a new recomp comes out for a game you own'))}
          ${sw('backup_before_update', t('Back up saves before updates'), t('The newest five copies of each game\'s saves'))}
          ${sw('hide_ai', t('Hide AI-assisted recomps'), t('They won\'t show in Discover'))}
          ${sw('discord', t('Discord status'), t('Show the game you\'re playing on your Discord profile'), S.settings.discord !== false)}
          <button type="button" class="tv-item" data-f data-key="s:storage" data-tvact="storage"><span class="k">${esc(t('Storage'))}<small>${esc(t('How much room each game takes, and uninstalling'))}</small></span>${svg('chevR', 30)}</button>
          <button type="button" class="tv-item" data-f data-key="s:saves" data-tvact="saves"><span class="k">${esc(t('Saves'))}<small>${esc(t('Every game\'s save copies, to put an older one back'))}</small></span>${svg('chevR', 30)}</button>
          ${head(t('Controller'))}
          ${(() => { const g = (S.settings.ui || {}).tv_glyphs; const v = g === 'ps' ? 'PlayStation' : g === 'xbox' ? 'Xbox' : g === 'nintendo' ? 'Nintendo' : 'Automatic'; return `<button type="button" class="tv-item" data-f data-key="s:glyphs" data-tvact="glyphs" data-seg="1" data-val="${esc(t(v))}"><span class="k">${esc(t('Button Icons'))}<small>${esc(t('Which controller\'s buttons are shown. Automatic follows the one you use.'))}</small></span><span class="tv-seg">${[N_('Automatic'), 'Xbox', 'PlayStation', 'Nintendo'].map((n) => `<span class="${n === v ? 'on' : ''}">${esc(t(n))}</span>`).join('')}</span></button>`; })()}
          <button type="button" class="tv-item" data-f data-key="s:buttons"><span class="k">${esc(t('Buttons'))}<small>${esc(t('What each button does'))}</small></span></button>
          ${head(t('Big Picture mode'))}
          <button type="button" class="tv-item" data-f data-key="s:leave" data-tvact="leave"><span class="k">${esc(t('Leave Big Picture mode'))}<small>${esc(t('Back to the desktop view, for mouse and keyboard'))}</small></span><span class="tv-pill on">${esc(t('Leave'))}</span></button>
          ${head(t('About'))}
          <button type="button" class="tv-item" data-f data-key="s:support" data-tvact="support">${HEART}<span class="k">${esc(t('Support RecompLauncher'))}<small>${esc(t('Help keep it running and growing, on Ko-fi'))}</small></span>${svg('chevR', 30)}</button>
          <button type="button" class="tv-item" data-f data-key="s:about" data-tvact="about"><span class="k">${esc(t('About RecompLauncher'))}<small>${esc(t('Who makes it, and where to find the Discord'))}</small></span>${svg('chevR', 30)}</button>
        </div><aside class="tv-setpanel" id="tv-setpanel" aria-live="polite"></aside></div>`, 'top')}
      ${hints([['A', t('Change')], ['dpad', t('Pick')], ['B', t('Back')], ['LB+RB', t('Sections')]])}`;
  }

  // Storage: every installed game by the room it takes, biggest first. A on one to uninstall it.
  function pageStorage() {
    if (!TV.sizes) { TV.sizes = 'loading'; invoke('game_sizes').then((r) => { TV.sizes = r || { sizes: {}, drives: [] }; if (TV.on && TV.view === 'storage') draw(true); }).catch(() => { TV.sizes = { sizes: {}, drives: [] }; if (TV.on && TV.view === 'storage') draw(true); }); }
    const z = TV.sizes === 'loading' ? null : TV.sizes;
    const list = installed().map((x) => ({ a: x.a, g: x.g, size: z ? z.sizes[x.g.id] : null })).sort((p, q) => (q.size || 0) - (p.size || 0) || p.a.name.localeCompare(q.a.name));
    const max = Math.max(1, ...list.map((x) => x.size || 0)), total = list.reduce((n, x) => n + (x.size || 0), 0);
    const drives = z ? z.drives.map((d) => tf('{drive} has {free} free', { drive: d.drive, free: fmtB(d.free) })).join(' · ') : '';
    const items = list.map((x) => `<button type="button" class="tv-item tv-stor" data-f data-key="st:${esc(x.g.id)}" data-tvact="stor" data-id="${esc(x.g.id)}">${x.a.cover ? `<img src="${esc(media(x.a.cover))}" alt="">` : '<span class="ph"></span>'}<span class="k">${esc(x.a.name)}<small>${esc(x.g.path)}</small></span><span class="tv-stor-r"><b>${x.size == null ? esc(z ? '—' : t('Loading…')) : esc(fmtB(x.size))}</b><i><em style="width:${x.size ? Math.max(2, Math.round((x.size / max) * 100)) : 0}%"></em></i></span></button>`).join('');
    return `${backdrop(null)}${gameBar()}
      ${region(`${pageHead(t('Storage'), z ? [tf('Your games take {size}', { size: fmtB(total) || '0 KB' }), drives].filter(Boolean).join(' · ') : t('Loading…'))}
        <div class="tv-list">${items || `<p class="tv-empty">${esc(t('Nothing installed yet.'))}</p>`}</div>`)}
      ${hints([['A', t('Options')], ['B', t('Back')]])}`;
  }

  // Saves: every game with save copies, newest first.
  function pageSavesAll() {
    if (!TV.savesAll) { TV.savesAll = 'loading'; invoke('saves_overview').then((o) => { TV.savesAll = o || { games: [] }; if (TV.on && TV.view === 'saves') draw(true); }).catch((e) => { TV.savesAll = { error: String(e && e.message ? e.message : e), games: [] }; if (TV.on && TV.view === 'saves') draw(true); }); }
    const o = TV.savesAll === 'loading' ? null : TV.savesAll;
    const games = o ? (o.games || []).map((x) => ({ ...x, g: S.library[x.id] || {}, a: byId(x.id) })).filter((x) => x.g.name || x.a) : [];
    const withSaves = games.filter((x) => x.copies).sort((p, q) => ((q.newest || {}).when || 0) - ((p.newest || {}).when || 0));
    const none = games.length - withSaves.length;
    const items = withSaves.map((x) => { const a = x.a || { id: x.id, name: x.g.name }; return `<button type="button" class="tv-item tv-stor" data-f data-key="sv:${esc(x.id)}" data-tvact="gsaves" data-id="${esc(x.id)}">${a.cover ? `<img src="${esc(media(a.cover))}" alt="">` : '<span class="ph"></span>'}<span class="k">${esc(a.name)}<small>${esc(tf('Last saved {when}', { when: ago(x.newest.when) }))}${x.cloudOn === true ? ' · ' + esc(t('In the cloud')) : ''}</small></span><span class="tv-stor-r"><b>${esc(tn(x.copies, '1 copy', '{n} copies'))}</b><small>${esc(fmtB(x.size))}</small></span></button>`; }).join('');
    return `${backdrop(null)}${gameBar()}
      ${region(`${pageHead(t('Saves'), t('After every session the launcher keeps a copy of each game\'s saves. Pick a game to put an older one back.'))}
        ${o && o.error ? `<p class="tv-empty">${esc(t(o.error))}</p>` : ''}
        <div class="tv-list">${!o ? `<p class="tv-empty">${esc(t('Loading…'))}</p>` : items || `<p class="tv-empty">${esc(t('Install a game and play it once. Its saves are copied here after every session.'))}</p>`}</div>
        ${none ? `<p class="tv-note">${esc(tn(none, '1 more game has no saves yet. Play it once and its saves show up here.', '{n} more games have no saves yet. Play them once and their saves show up here.'))}</p>` : ''}`)}
      ${hints([['A', t('Open')], ['B', t('Back')]])}`;
  }

  // One game's saves: cloud on or off, a copy now, and every copy to put back.
  async function loadGameSaves(id) {
    try { TV.saves[id] = await invoke('saves_info', { id }); } catch (e) { TV.saves[id] = { error: String(e && e.message ? e.message : e) }; }
    if (TV.on && TV.view === 'gsaves' && TV.arg === id) draw(true);
  }
  function pageGameSaves(id) {
    const a = libAppOf(id), g = S.library[id] || {};
    const s0 = TV.saves[id];
    if (s0 === undefined) { TV.saves[id] = 'loading'; loadGameSaves(id); }
    const sv = s0 && s0 !== 'loading' ? s0 : null;
    const copy = (c, k, v, now) => { const when = c.when || c.made; return `<button type="button" class="tv-item" data-f data-key="gc:${k}:${esc(v)}" data-tvact="gs-restore" data-id="${esc(id)}" data-${k}="${esc(v)}"><span class="k">${esc(whenText(when))}${now ? ` <em class="tv-now">${esc(t('Same as this PC'))}</em>` : ''}<small>${esc([c.device, c.files ? tn(c.files, '1 file', '{n} files') : '', fmtB(c.size)].filter(Boolean).join(' · '))}</small></span><span class="tv-pill">${esc(t('Put back'))}</span></button>`; };
    let body = `<p class="tv-empty">${esc(t('Loading…'))}</p>`;
    if (sv && sv.error) body = `<p class="tv-empty">${esc(t(sv.error))}</p>`;
    else if (sv) {
      const on = sv.cloudOn === true;
      body = `<div class="tv-list">
        ${sv.signedIn ? `<button type="button" class="tv-item" data-f data-key="gs:cloud" data-tvact="gs-cloud" data-id="${esc(id)}" data-sw="${on ? 1 : 0}"><span class="k">${esc(t('Cloud saves'))}<small>${esc(on ? t('On. After you play, its saves go to your account.') : t('Off. Its saves only stay on this PC.'))}</small></span><span class="switch" aria-checked="${on}"></span></button>` : ''}
        <button type="button" class="tv-item" data-f data-key="gs:now" data-tvact="gs-backup" data-id="${esc(id)}"${S.running.has(id) ? ' disabled' : ''}>${svg('upload')}<span class="k">${esc(t('Back up now'))}<small>${esc(t('A copy of its saves as they are right now'))}</small></span></button>
        <h3 class="tv-lh">${esc(t('On this PC'))}</h3>
        ${sv.local.length ? sv.local.map((c) => copy(c, 'name', c.name, false)).join('') : `<p class="tv-note">${esc(g.last_played ? t('No saves found yet. If it saves somewhere unusual, its developer can say where in recomplauncher.json.') : t('Play it once and its saves are copied here after.'))}</p>`}
        ${on && Array.isArray(sv.cloud) ? `<h3 class="tv-lh">${esc(t('In the cloud'))}</h3>${sv.cloud.length ? sv.cloud.map((c) => copy(c, 'v', c.v, c.hash === sv.synced)).join('') : `<p class="tv-note">${esc(t('Nothing in the cloud yet. It goes up after you play.'))}</p>`}` : ''}
        <h3 class="tv-lh">${esc(t('Where it saves'))}</h3>
        ${(sv.spots || []).map((p, i) => `<button type="button" class="tv-item" data-f data-key="gs:spot:${i}" data-tvact="gs-open" data-id="${esc(id)}">${svg('folder')}<span class="k tv-path">${esc(p.path)}<small>${esc(t(FROM[p.from] || ''))}${p.exists ? '' : ' · ' + esc(t('Not there yet'))}</small></span></button>`).join('')}
      </div>`;
    }
    return `${backdrop(a, 'game')}${gameBar()}
      ${region(`${pageHead(tf('Saves for {game}', { game: a.name }), t('After every session the launcher keeps a copy. Put an older one back with A.'))}${body}`)}
      ${hints([['A', t('Select')], ['B', t('Back')]])}`;
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
      !g && needsSetup(a) ? btn('g-sreq', 'setupreq', esc(t('Setup Required')), { cls: 'warn-o', id, icon: 'wrench' }) : '',
      devOf(a) ? btn('g-follow', 'follow', esc(follows().includes(devOf(a).slug) ? t('Following') : t('Follow')), { id, icon: follows().includes(devOf(a).slug) ? 'check' : 'user' }) : '',
      verGroup(a) ? btn('g-vers', 'versions', esc(tn(verGroup(a).list.length, '1 version', '{n} versions')), { id, icon: 'layers' }) : '',
      g ? btn('g-shots', 'shots', esc(t('Screenshots')), { id, icon: 'shot' }) : '',
      g && (su || looksHere()) ? btn('g-set', 'gsettings', esc(t('Game settings')) + (su && su.beta && su.settings && su.settings.length ? `<b class="tv-beta-s">${esc(t('Beta'))}</b>` : ''), { id, icon: 'gear' }) : '',
      g && modsOk ? btn('g-mods', 'mods', esc(t('Mods')), { id, icon: 'puzzle' }) : '',
      btn('g-more', 'more', esc(t('Options')), { id, g: 'x' }),
    ].filter(Boolean).join('');
    const ab = a.about && (a.about.summary || aboutFacts(a.about).length) ? a.about : null;
    const score = ab && ab.rating !== null && ab.rating !== undefined && ab.rating !== '' && Number.isFinite(Number(ab.rating)) ? Math.round(Number(ab.rating)) : null;
    const about = ab ? `<section class="tv-block"><h3>${esc(t('About the game'))}${score !== null ? ` <small class="tv-score"><b>${score}</b>${esc(t('Critic and player score'))}</small>` : ''}</h3><div class="tv-panel tv-igdb" data-f data-key="g-about">${ab.summary ? `<p class="tv-about" dir="auto">${esc(String(ab.summary).split(/\n\s*\n/)[0])}</p>` : ''}${aboutFacts(ab).length ? `<dl>${aboutFacts(ab).map(([k, v]) => `<div><dt>${esc(k)}</dt><dd>${esc(v)}</dd></div>`).join('')}</dl>` : ''}<p class="tv-src">${esc(t('Game info from IGDB'))}</p></div></section>`
      : info ? `<section class="tv-block"><h3>${esc(tf('About {title}', { title: info.title }))}</h3><p class="tv-about">${esc(info.text.split('\n\n')[0])}</p><p class="tv-src">${esc(t('From Wikipedia'))}</p></section>`
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
          ${titleOf(a)}
          ${a.game ? `<p class="tv-based">${DISC_S.replace(/14/g, '24')}<span>${htmlIn(t('Based on {game}'), { game: `<b>${esc(a.game)}</b>` })}</span>${needsSetup(a) && !setupDone(id) ? `<span class="tv-easy sreq">${svg('wrench', 20)}${esc(t('Setup Required'))}</span>` : S.easy.has(id) ? `<span class="tv-easy">${esc(t('Easy Setup'))}</span>` : ''}</p>` : ''}
          ${g && g.last_played ? `<p class="tv-meta">${esc(tf('{time} played · last played {when}', { time: playedFor(g), when: ago(g.last_played) }))}</p>` : ''}
          <div class="tv-acts">${acts}</div>
          ${tagsOf(a).length ? `<div class="tv-acts tv-tags" aria-label="${esc(t('Tags'))}">${tagsOf(a).map((tg) => `<button type="button" class="tv-tag" data-f data-key="tag:${esc(tg)}" data-tvact="tag" data-tag="${esc(tg)}">${esc(tg)}</button>`).join('')}</div>` : ''}
          ${about}${checkBlock}${shotRow}
        </div>
        ${aside}
      </div>${(() => { const like = moreLike(a); return like.length ? row('like', t('More like this'), like.map((x) => poster(x, 'm:' + x.id)).join('')) : ''; })()}`, 'game')}
      ${hints([['A', t('Select')], ['X', t('Options')], ['B', t('Back')]])}`;
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
    const win = looksHere();
    const li = win ? S.looks[id] : null;
    if (win && li === undefined) loadLooks(id);
    const LD = window.Looks && Looks.defs();
    const waiting = su === undefined || su === 'loading' || (win && !(window.Looks && Looks.failed()) && (li === undefined || li === 'loading' || !LD));
    const prof = su && su !== 'loading' ? su : null;
    const look = li && li.ok && LD ? LD.looks.find((l) => l.id === li.look) : null;
    const lookRows = li && li.ok && LD ? [
      `<button type="button" class="tv-item" data-f data-key="gs:__look" data-gset="__look" data-id="${esc(id)}"><span class="k">${esc(t('Look'))}<small>${esc(S.os === 'linux' ? t('Drawn over the game by vkBasalt.') : t('Drawn over the game by ReShade.'))}</small></span><span class="tv-cycle">${svg('chevL', 22)}<b>${esc(look ? t(look.name) : t('Original'))}</b>${svg('chevR', 22)}</span></button>`,
      `<button type="button" class="tv-item${look ? '' : ' off'}" data-f data-key="gs:__amount" data-gset="__amount" data-id="${esc(id)}"${look ? '' : ' disabled'}><span class="k">${esc(t('Amount'))}<small>${esc(t('How strong the look is.'))}</small></span><span class="tv-cycle">${svg('chevL', 22)}<b>${esc(tf('{pct}%', { pct: li.amount }))}</b>${svg('chevR', 22)}</span></button>`,
    ] : [];
    const items = waiting ? `<p class="tv-empty">${esc(t('Loading…'))}</p>` : [
      prof && prof.needs ? `<button type="button" class="tv-item" data-f data-key="gs:copy" data-tvact="setup" data-id="${esc(id)}"><span class="k">${esc(t('Your copy of the game'))}<small>${esc(prof.copy ? baseName(prof.copy) : prof.needs)}</small></span><span class="tv-pill${prof.copy ? ' on' : ''}">${esc(prof.copy ? t('Change') : t('Insert it'))}</span></button>` : '',
      ...lookRows,
      ...(prof ? prof.settings : []).map((o) => `<button type="button" class="tv-item${o.applies ? '' : ' off'}" data-f data-key="gs:${esc(o.id)}" data-gset="${esc(o.id)}" data-id="${esc(id)}"${o.applies ? '' : ' disabled'}><span class="k">${esc(o.label)}<small>${esc(o.help)}</small></span><span class="tv-cycle">${svg('chevL', 22)}<b>${esc(o.choices[o.current] || '')}</b>${svg('chevR', 22)}</span></button>`),
      ...(prof ? prof.actions : []).map((x) => `<button type="button" class="tv-item" data-f data-key="ga:${esc(x.id)}" data-tvact="gaction" data-k="${esc(x.id)}" data-id="${esc(id)}"><span class="k">${esc(x.label)}<small>${esc(x.help)}</small></span><span class="tv-pill on">${esc(x.kind === 'clear' ? t('Clear') : t('Open'))}</span></button>`),
    ].join('') || `<p class="tv-empty">${esc(t('Nothing to set for this game.'))}</p>`;
    const sub = prof && prof.settings.length ? (prof.waiting ? t('Play it once first, then change them here.') : t('Saved in the game\'s own settings, so they\'re the same in its own launcher.')) : S.os === 'linux' ? t('Looks are drawn over the game. Press Home while you play to turn one off and on.') : t('Looks are drawn over the game. Press Home while you play to fine-tune one.');
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
  function sheet(title, items, cls, head) {
    TV.sheet = { items, prev: (focused() || {}).dataset?.key };
    const el = document.createElement('div');
    el.className = 'tv-sheet' + (cls ? ' ' + cls : '');
    el.innerHTML = `<div class="tv-sheet-in">${head || `<h3>${esc(title)}</h3>`}${items.map((x, i) => `${x.gap ? '<hr class="tv-sh-gap">' : ''}<button type="button" class="tv-item${x.cls ? ' ' + x.cls : ''}" data-f data-key="sheet:${i}" data-sheet="${i}">${x.icon ? svg(x.icon) : ''}<span class="k">${esc(x.label)}${x.sub ? `<small>${esc(x.sub)}</small>` : ''}</span></button>`).join('')}</div>`;
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
  // X on any game: everything you can do with it, in a sheet from the side.
  function gameSheet(id) {
    const a = libAppOf(id), g = S.library[id];
    const onPage = GAMEVIEWS.includes(TV.view) && TV.arg === id;
    const pinned = pins().includes(id);
    const m = mainItem(a);
    const items = [
      ...(m ? [m] : []),
      ...(TV.view !== 'game' || !onPage ? [{ label: t('Game page'), sub: a.game ? tf('Based on {game}', { game: a.game }) : '', icon: 'page', run: () => go('game', id) }] : []),
      { label: pinned ? t('Unpin from Home') : t('Pin to Home'), sub: pinned ? t('It comes off your Home row') : t('Keep it on Home, in the order you like'), icon: 'pin', run: () => togglePin(id) },
      ...(pinned && TV.view === 'home' && homePinned() && $(`.tv-dk[data-dock="${CSS.escape(id)}"]`, stage()) ? [{ label: t('Move'), sub: t('Change where it sits on Home'), icon: 'move', run: () => startSort(id) }] : []),
      ...(TV.view === 'game' && onPage && verGroup(a) ? [{ label: tn(verGroup(a).list.length, '1 version', '{n} versions'), sub: t('Pick which one this page is for'), icon: 'layers', run: () => pickVersion(id) }] : []),
      ...(g && !S.running.has(id) ? [{ label: t('Start With'), sub: t('Pick which of its programs starts this time'), icon: 'play', run: () => { closeSheet(); setTimeout(() => window.startWith(id), 250); } }] : []),
      ...(g ? [
        { gap: true, label: t('Saves'), sub: t('Back them up, or put an older one back'), icon: 'save', run: () => go('gsaves', id) },
        ...((S.setup[id] && S.setup[id] !== 'loading') || looksHere() ? [{ label: t('Game settings'), sub: t('Its own settings, and Looks'), icon: 'gear', run: () => go('gsettings', id) }] : []),
        { label: t('Screenshots'), icon: 'shot', run: async () => { await loadAllShots(); go('shots', id); } },
        ...(S.mods[id] && S.mods[id] !== 'loading' ? [{ label: t('Mods'), icon: 'puzzle', run: () => go('mods', id) }] : []),
        ...(S.running.has(id) ? [{ label: t('Close the game'), sub: t('Anything you haven\'t saved is lost'), icon: 'stop', run: () => invoke('stop', { id }) }] : []),
        { gap: true, ...(S.steam ? { label: t('Add to Steam'), sub: t('With its art'), icon: 'steam', run: () => addToSteam(id, false) } : { label: t('Add to desktop'), sub: t('A shortcut that starts it'), icon: 'desk', run: async () => { try { await invoke('desktop_shortcut', { id }); toast(tf('{name} is on your desktop.', { name: g.name })); } catch (e) { fail(e); } } }) },
        ...(S.steam ? [{ label: t('Add to desktop'), sub: t('A shortcut that starts it'), icon: 'desk', run: async () => { try { await invoke('desktop_shortcut', { id }); toast(tf('{name} is on your desktop.', { name: g.name })); } catch (e) { fail(e); } } }] : []),
        ...(S.account && S.account.signedIn ? [{ label: g.cloud ? t('Turn off cloud saves') : t('Turn on cloud saves'), sub: g.cloud ? t('Its saves stay on this PC from now on') : t('Its saves go to your account after you play'), icon: 'cloud', run: async () => { try { S.library[id] = await invoke('saves_cloud', { id, on: !g.cloud }); toast(g.cloud ? t('Cloud saves are off for it. What\'s in the cloud stays there.') : t('Cloud saves are on for it.')); } catch (e) { fail(e); } } }] : []),
        ...(window.tvBetaItem && window.tvBetaItem(id) ? [window.tvBetaItem(id)] : []),
        { label: t('Older releases'), sub: t('Go back to an older one, or forward again'), icon: 'history', run: () => versionsSheet(id) },
        { label: t('Open its folder'), sub: g.path, icon: 'folder', run: () => invoke('open_folder', { id }).catch(fail) },
        ...(!S.running.has(id) && !S.jobs[id] ? [{ gap: true, label: t('Uninstall'), sub: t('Take it off this PC'), icon: 'bin', cls: 'danger', run: () => tvUninstall(id) }] : []),
      ] : []),
    ];
    const st = S.running.has(id) ? t('Playing now') : g ? timeLine(g) : a.game ? tf('Based on {game}', { game: a.game }) : '';
    const head = `<div class="tv-sh-head">${a.cover ? `<img src="${esc(media(a.cover))}" alt="">` : ''}<div><b>${esc(a.name)}</b>${st ? `<span>${esc(st)}</span>` : ''}</div></div>`;
    sheet(a.name, items, 'tv-gsheet', head);
    TV.sheet.game = id;
  }
  async function togglePin(id) {
    const now = pins(), on = now.includes(id);
    const next = on ? now.filter((x) => x !== id) : now.concat(id);
    const first = !on && !now.length && !homePinned();
    await saveUi(first ? { tv_pins: next, tv_home: 'pinned' } : { tv_pins: next });
    const name = libAppOf(id).name;
    toast(on ? tf('{name} is off your Home row.', { name }) : first ? tf('{name} is pinned. Home now shows your pinned games; Settings switches it back.', { name }) : tf('{name} is pinned to Home.', { name }));
    if (TV.view === 'home') { TV.focusKey = 'c:' + id; draw(true); }
  }
  // Moving a pinned game along the dock: left and right move it, A keeps it there, B puts it back.
  function startSort(id) {
    TV.sort = { id, was: pins() };
    TV.focusKey = 'c:' + id;
    sfx('select');
    draw(true);
  }
  function sortMove(dir) {
    const list = pins(), i = list.indexOf(TV.sort.id), j = i + dir;
    if (i < 0 || j < 0 || j >= list.length) return;
    [list[i], list[j]] = [list[j], list[i]];
    // The others slide aside instead of jumping (where each was, to where it is).
    const before = {};
    for (const e of $$('.tv-dk', stage())) before[e.dataset.dock] = e.getBoundingClientRect().left;
    S.settings.ui = Object.assign({}, S.settings.ui, { tv_pins: list });
    TV.focusKey = 'c:' + TV.sort.id;
    draw(true);
    for (const e of $$('.tv-dk', stage())) {
      const dx = (before[e.dataset.dock] ?? 0) - e.getBoundingClientRect().left;
      if (dx) e.animate([{ translate: `${dx / (TV.k || 1)}px 0` }, { translate: '0 0' }], { duration: 220, easing: 'cubic-bezier(.2,.8,.2,1)' });
    }
    sfx('move');
  }
  async function endSort(keep) {
    const s0 = TV.sort;
    TV.sort = null;
    if (keep) await saveUi({ tv_pins: pins() });
    else S.settings.ui = Object.assign({}, S.settings.ui, { tv_pins: s0.was });
    sfx(keep ? 'select' : 'back');
    TV.focusKey = 'c:' + s0.id;
    draw(true);
  }
  // Uninstall, from a controller: the same choices as on the desktop.
  function tvUninstall(id) {
    const g = S.library[id];
    if (!g) return;
    if (S.running.has(id)) return toast(t('Close the game first.'), true);
    const run = (del) => async () => {
      let r = {};
      try { r = (await invoke('uninstall', { id, deleteFiles: del })) || {}; } catch (e) { await refreshState(); draw(true); return fail(e); }
      if (window.usageUninstall) window.usageUninstall(id);
      delete S.updates[id];
      if (TV.sizes && TV.sizes.sizes) delete TV.sizes.sizes[id];
      await refreshState();
      render(true);
      if (r.left) toast(tf('{name} is uninstalled, but a few of its files are still in use and stayed in {path}. Close whatever is using them (or restart), then delete that folder.', { name: g.name, path: r.path }), true);
      else if (r.uninstaller) toast(tf('{name} is uninstalled. Its own uninstaller is open to remove the game from where it installed it.', { name: g.name }));
      else toast(tf('{name} is uninstalled.', { name: g.name }));
      draw(true);
    };
    const own = ownFolder(g);
    sheet(tf('Uninstall {name}?', { name: g.name }), [
      { label: t('Keep it'), sub: t('Nothing changes'), icon: 'chevL', run: () => {} },
      ...(own
        ? [{ label: t('Uninstall and delete its folder'), sub: tf('{path}, with the saves and settings inside it', { path: g.path }), icon: 'bin', cls: 'danger', run: run(true) },
          { label: t('Uninstall, keep its folder'), sub: t('It comes out of your Library and its files stay'), icon: 'folder', run: run(false) }]
        : [{ label: t('Uninstall'), sub: tf('Its folder stays where it is: {path}', { path: g.path }), icon: 'bin', cls: 'danger', run: run(false) }]),
    ]);
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
    const back = prev && $(`.tv-stage:not(.leaving) > :not(.tv-sheet) [data-key="${CSS.escape(prev)}"]`, root);
    if (back) focus(back, true);
  }

  // ---------- picking a file or folder with a controller ----------
  // The launcher's own browser, full screen: places on the left, the folder on the right. A opens or
  // picks, B goes up a folder (and leaves at the top), X uses the open folder, Y opens Options: copy
  // or cut what's focused, paste it into the open folder, or jump to places.
  const TB = { el: null };
  window.tvBrowse = async (opts) => {
    let places;
    try { places = await invoke('fs_places'); } catch (e) { return null; }
    if (TB.el) tbClose(null);
    return new Promise((resolve) => {
      const el = document.createElement('div');
      el.className = 'tv-sheet tv-browse';
      stage().append(el);
      Object.assign(TB, { el, opts, places, path: '', list: null, games: {}, resolve, prevSheet: TV.sheet, prevKey: (focused() || {}).dataset?.key, n: 0, menu: null, busy: '' });
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
    if (!v) v = { path, parent: null, entries: [], denied: true, games: {} };
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
    // Options (Y) on what's focused: copy or cut it, paste what was copied into this folder.
    let menu = '';
    const M = TB.menu;
    if (M) {
      const mi = (key, label, sub, run) => item('tb:m:' + key, `<span class="k">${esc(label)}${sub ? `<small>${esc(sub)}</small>` : ''}</span>`, run);
      const name = M.path ? M.path.replace(/[\\/]+$/, '').split(/[\\/]/).pop() : '';
      const into = TB.path.replace(/[\\/]+$/, '').split(/[\\/]/).pop() || TB.path;
      menu = `<div class="tb-menu"><h3>${esc(t('Options'))}</h3>${name ? `<p>${esc(name)}</p>` : ''}${[
        M.path ? mi('copy', t('Copy'), t('Then paste it into another folder'), () => { TB.clip = { path: M.path, cut: false, name }; tbMenuClose(); toast(tf('{name} copied. Open a folder and press Y to paste it.', { name })); }) : '',
        M.path ? mi('cut', t('Cut'), t('Move it to another folder'), () => { TB.clip = { path: M.path, cut: true, name }; tbMenuClose(); toast(tf('{name} cut. Open a folder and press Y to paste it.', { name })); }) : '',
        TB.clip && !L.denied ? mi('paste', TB.clip.cut ? t('Move Here') : t('Paste Here'), tf('{name} into {folder}', { name: TB.clip.name, folder: into }), () => tbPaste()) : '',
        mi('places', t('Places'), t('Your folders and drives'), () => { TB.menu = null; tbDraw(); const p = $('.tb-side .tv-item.on', el) || $('.tb-side .tv-item', el); if (p) focus(p); }),
      ].join('')}</div>`;
    }
    // Redrawn in place (the Options menu, a paste): both lists stay where they were scrolled to.
    const keep = TB.path === TB.drawn ? [$('.tb-side', el), $('.tb-list', el)].map((x) => (x ? x.scrollTop : 0)) : null;
    TB.drawn = TB.path;
    el.innerHTML = `<div class="tb-in"><aside class="tb-side">${side}</aside><section class="tb-main"><div class="tb-head"><h2>${esc(o.title || t('Choose'))}</h2><p class="${TB.busy ? 'tb-busy' : ''}">${esc(TB.busy || TB.path)}</p></div>${useHere}${refresh}<div class="tb-list">${list}</div></section></div>${menu}
      ${hints(M ? [['A', t('Select')], ['B', t('Back')]] : [['A', t('Open')], ['B', L.parent ? t('Up a folder') : t('Back')]].concat(o.directory ? [['X', t('Use this folder')]] : [], [['Y', t('Options')]]))}`;
    TV.sheet = {
      items, keep: true, browse: true, prev: TB.prevKey,
      back: () => { if (TB.menu) { tbMenuClose(); return true; } if (TB.list && TB.list.parent) tbGo(TB.list.parent); else tbClose(null); return true; },
      keys: (name, cur) => {
        if (TB.menu) {
          // Up and down stay in the menu; A picks, B closes it.
          if (name === 'up' || name === 'down') {
            const its = $$('.tb-menu .tv-item', el), n = its[its.indexOf(cur) + (name === 'down' ? 1 : -1)];
            if (n) { focus(n); sfx('move'); }
            return true;
          }
          return !['a', 'b'].includes(name);
        }
        if (name === 'x' && o.directory) { tbPick(TB.path); return true; }
        if (name === 'y') {
          const f = focused(), k = (f && f.dataset.key) || '';
          TB.menu = { path: k.startsWith('tb:e:') ? k.slice(5) : '', back: k };
          sfx('open'); tbDraw();
          return true;
        }
        // The bumpers and Menu would switch pages under it: they do nothing here.
        return ['lb', 'rb', 'menu', 'x'].includes(name);
      },
    };
    if (keep) { const [a, b] = [$('.tb-side', el), $('.tb-list', el)]; if (a) a.scrollTop = keep[0]; if (b) b.scrollTop = keep[1]; }
    const f = (M && $('.tb-menu .tv-item', el)) || (focusKey && $(`[data-key="${CSS.escape(focusKey)}"]`, el)) || $('.tb-list .tv-item', el) || $('.tb-main .tv-item', el) || $('.tb-side .tv-item', el);
    if (f) focus(f, true);
  }
  function tbMenuClose() {
    const back = TB.menu && TB.menu.back;
    TB.menu = null;
    sfx('back');
    tbDraw(back);
  }
  // Paste what was copied or cut into the open folder; a big disc image takes a while.
  async function tbPaste() {
    const c = TB.clip, dir = TB.path;
    if (!c || !dir) return;
    TB.menu = null;
    TB.busy = c.cut ? tf('Moving {name}…', { name: c.name }) : tf('Copying {name}…', { name: c.name });
    tbDraw();
    try {
      const to = await invoke('fs_paste', { from: c.path, toDir: dir, cut: c.cut });
      if (c.cut) TB.clip = null;
      TB.busy = '';
      toast(c.cut ? tf('Moved {name} here.', { name: c.name }) : tf('Copied {name} here.', { name: c.name }));
      if (TB.el) tbGo(TB.path, 'tb:e:' + to);
    } catch (e) {
      TB.busy = '';
      fail(e);
      if (TB.el) tbDraw();
    }
  }

  // ---------- setting a game up with the player's copy, full screen ----------
  const TS = { id: null, stage: 'pick', step: '', steps: {}, q: [], at: 0, path: '', extras: false, result: null, err: '' };
  async function tvSetup(id) {
    if (TS.id && TS.stage === 'run' && TS.id !== id) return toast(t('Finish the setup that\'s running first.'));
    if (S.setup[id] === undefined || S.setup[id] === 'loading') { try { S.setup[id] = await invoke('game_setup', { id }); } catch (e) { S.setup[id] = null; } }
    const su = S.setup[id];
    if (!su || !su.needs) return toast(t('This game asks for your copy itself the first time it starts.'));
    if (TV.sheet) closeSheet();
    Object.assign(TS, { id, stage: su.copy && su.update && !su.update_copy && !su.update_optional ? 'update' : 'pick', step: '', steps: {}, q: [], at: 0, path: '', extras: false, result: null, err: '', uerr: '' });
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
    const pickIt = async (folder) => { const p = await pickFile(folder ? { title: t('The game\'s folder'), directory: true } : copyPickOpts(TS.id)).catch(fail); if (p) tsBegin(typeof p === 'string' ? p : p.path); else tsDraw(); };
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
    } else if (TS.stage === 'update') {
      const pickU = async (folder) => {
        const p = await pickFile(folder ? { title: t('The update\'s folder'), directory: true } : { title: t('The game\'s update'), directory: false, multiple: false, filters: [{ name: t('All files'), extensions: ['*'] }] }).catch(fail);
        if (!p) return tsDraw();
        try { await invoke('set_update_copy', { id, path: typeof p === 'string' ? p : p.path }); } catch (e) { TS.uerr = String(e && e.message ? e.message : e); sfx('back'); return tsDraw(); }
        delete S.setup[id];
        try { S.setup[id] = await invoke('game_setup', { id }); } catch (e) { S.setup[id] = null; }
        TS.uerr = ''; TS.stage = 'ready'; TS.result = TS.result || { text: t('Checked') }; sfx('select'); tsDraw();
      };
      body = `<div class="bigdisc">${label}</div><div class="tt"><h2>${esc(t('Add the Game\'s Update'))}</h2><p>${esc(su.update || '')}</p>${TS.uerr ? `<p class="err">${esc(t(TS.uerr))}</p>` : ''}</div>
        <div class="opts">${su.update_file ? it(t('Choose the Update File'), t('The file, as it came'), () => pickU(false), 'main') : ''}${su.update_folder ? it(t('Choose the Update Folder'), t('A folder with its files'), () => pickU(true), su.update_file ? '' : 'main') : ''}${su.update_optional ? it(t('Skip for Now'), '', () => { TS.stage = 'ready'; setTimeout(tsDraw, 0); }) : ''}</div>`;
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
      if (ev.step === 'done') {
        TS.result = ev;
        const sid = TS.id;
        invoke('game_setup', { id: sid }).then((x) => { S.setup[sid] = x; }).catch(() => {}).finally(() => {
          if (TS.id !== sid) return;
          const su = S.setup[sid];
          TS.stage = su && su.update && !su.update_copy ? 'update' : 'ready';
          sfx('select'); tsDraw();
        });
        return;
      }
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
  // About: who makes it, where to find it, and the Discord, with codes to scan from a phone.
  const HEART = '<svg width="22" height="22" viewBox="0 0 24 24" aria-hidden="true"><path fill="#FF5E7A" d="M12 21s-7.5-4.6-9.6-9.3C.9 8.2 3 4.5 6.6 4.5c2.1 0 3.5 1.2 4.4 2.6.9-1.4 2.3-2.6 4.4-2.6 3.6 0 5.7 3.7 4.2 7.2C19.5 16.4 12 21 12 21z"/></svg>';
  function pageAbout() {
    return `${backdrop(null)}${bar()}
      ${region(`<div class="tv-about">
        <div class="tv-about-l">${MARK}<img class="tv-about-wm" src="img/${document.body.classList.contains('light') ? 'wordmark-dark' : 'wordmark'}.svg" alt="RecompLauncher">
          <p class="v">${esc(tf('Version {v}', { v: verName(S.version) }))}</p>
          <p>${esc(t('RecompLauncher has no game files and never downloads any. Every recomp comes from its developer\'s own GitHub, and you set it up with your own copy of the game.'))}</p>
          <p class="by">${esc(t('Made by Ripalda'))}</p>
          <button type="button" class="tv-btn" data-f data-key="ab:back" data-tvact="back">${glyph('b', 'tv-keyg')}<span>${esc(t('Back'))}</span></button></div>
        <div class="tv-qrs"><figure><img src="img/qr-discord.svg" alt=""><figcaption><b>${esc(t('Discord'))}</b><span>${esc(t('News, help and a place to send your logs'))}</span></figcaption></figure>
          <figure><img src="img/qr-site.svg" alt=""><figcaption><b>recomplauncher.com</b><span>${esc(t('Every recomp, and the newest launcher'))}</span></figcaption></figure>
          <figure><img src="img/qr-support.svg" alt=""><figcaption><b class="tv-heart">${HEART}${esc(t('Support Us'))}</b><span>ko-fi.com/recomp</span></figcaption></figure></div></div>`)}
      ${hints([['B', t('Back')]])}`;
  }
  const VIEWS = { home: pageHome, library: pageLibrary, achievements: pageAchTV, discover: pageDiscover, downloads: pageDownloads, settings: pageSettings, about: pageAbout, storage: pageStorage, saves: pageSavesAll };
  function draw(keep) {
    if (!TV.on || TV.booting) return;
    // A refresh while the camera is flying would cut it short: it waits until the game's page has landed.
    if (keep && TV.flying) { TV.later = true; return; }
    const prevKey = keep ? (focused() || {}).dataset?.key : null;
    const prevScroll = keep ? TV.scrollY || 0 : 0;
    const v = TV.view;
    const html = v === 'game' ? pageGame(TV.arg) : v === 'shots' ? pageShots(TV.arg) : v === 'mods' ? pageMods(TV.arg) : v === 'gsettings' ? pageGameSettings(TV.arg) : v === 'gsaves' ? pageGameSaves(TV.arg) : (VIEWS[v] || pageHome)();
    const anim = keep ? '' : TV.enter || 'in';
    if (!keep) TV.panelKey = null;
    TV.enter = null;
    const fresh = document.createElement('div');
    fresh.className = `tv-stage${anim ? ' tvx-' + anim : ''}`;
    fresh.innerHTML = html;
    const old = stage();
    // A screenshot or a sheet that's open stays open across a redraw.
    const keepers = keep && old ? $$(':scope > .tv-viewer, :scope > .tv-sheet', old) : [];
    // A new page comes in over the old one, which fades out under it (its bar and hints go at
    // once, so the new ones are simply there).
    for (const x of $$('.tv-stage.leaving', root)) x.remove();
    ambient();
    if (old && !keep && (anim === 'thru' || anim === 'thruback')) {
      // Flying into or out of a game: the page you were on stays while the camera moves, over the
      // next one on the way back and under it on the way in.
      old.classList.add('leaving');
      // It keeps its look exactly as it was (the cover you picked stays lit and turned); only its
      // names go, so nothing on the new page is mistaken for it.
      old.style.pointerEvents = 'none';
      for (const e of $$('[id]', old)) e.removeAttribute('id');
      for (const e of $$('[data-key]', old)) e.removeAttribute('data-key');
      if (anim === 'thru') old.after(fresh); else old.before(fresh);
    } else if (old) old.replaceWith(fresh); else root.prepend(fresh);
    // Moved over without playing their way in again (a sheet would slide in on every redraw).
    keepers.forEach((k) => { k.style.animation = 'none'; k.classList.add('kept'); fresh.append(k); });
    fit();
    fitAll();
    TV.swap = false;
    if (keep) setScroll(prevScroll, true);
    movePill();
    moveTabPill(!keep);
    if (!keep || TV.view !== 'home') setTone(GAMEVIEWS.includes(v) ? TV.arg : v === 'home' ? TV.hero : v === 'library' && (S.settings.ui || {}).tv_view === 'flow' ? TV.flowId : null);
    const pick = (k) => k && $(`.tv-stage:not(.leaving) > :not(.tv-sheet) [data-key="${CSS.escape(k)}"]`, root);
    // Where focus goes: back where it was, else the page's main button, its first item, or the
    // section's tab. If the page was still loading when it got the tab, its first item once it's there.
    const first = $('.tv-region .tv-btn.main[data-f]:not([disabled])', root) || $('.tv-region [data-f]:not([disabled])', root);
    let want = (TV.sheet && topSheet() && ($('[data-f].f', topSheet()) || $('[data-f]', topSheet()))) || (TV.parked && first) || pick(prevKey) || pick(TV.focusKey) || (TV.view === 'library' && TV.flowId && pick('L:' + TV.flowId)) || first;
    TV.parked = false;
    if (!want) { want = $('.tv-tab.on', root) || $('.tv-back', root) || $('[data-f]', root); TV.parked = true; }
    TV.focusKey = null;
    if (want) focus(want, true);
  }
  // The launcher's own backdrop, under every page: Aurora (slow light), Grid (a floor running to the
  // horizon) or Drift (specks of light), still on handhelds and with Lighter Launcher.
  function ambient() {
    const m = bgMode() === 'grid' ? 'grid' : bgMode() === 'drift' ? 'drift' : 'aurora';
    let el = $(':scope > .tv-ambient', root);
    if (!el) { el = document.createElement('div'); el.setAttribute('aria-hidden', 'true'); root.prepend(el); }
    const cls = `tv-ambient m-${m}`;
    if (el.className !== cls) { el.className = cls; el.innerHTML = m === 'grid' ? '<i class="sky"></i><i class="sun"></i><i class="floor"></i>' : m === 'drift' ? '<i class="d1"></i><i class="d2"></i><i class="d3"></i>' : '<i class="a1"></i><i class="a2"></i><i class="a3"></i>'; }
  }
  window.tvRefresh = () => soon();
  window.tvGoto = (view, id) => {
    if (!TV.on) return;
    if (view === 'shots') { loadAllShots().then(() => go('shots', id || null)); }
    else go(view, id || null);
  };

  const focused = () => $('.tv-stage:not(.leaving) [data-f].f', root);

  function focus(el, instant) {
    if (!el) return;
    const old = focused();
    if (old && old !== el) old.classList.remove('f');
    el.classList.add('f');
    growGrid(el);
    // Home: the backdrop and the hero follow the focused game.
    const hid = el.dataset.hero;
    if (hid && TV.view === 'home' && hid !== TV.hero && !el.closest('.tv-hero')) {
      TV.hero = hid;
      TV.heroBelow = !el.closest('.tv-dock');
      const a = libAppOf(hid);
      swapBg(a);
      setTone(hid);
      const hero = $('#tv-hero', root);
      if (hero) { hero.innerHTML = heroText(a, S.library[hid], true); hero.classList.remove('swap'); void hero.offsetWidth; hero.classList.add('swap'); fitAll(); }
    }
    if (TV.view === 'gsettings' || TV.view === 'settings') setPanel(el);
    if (el.classList.contains('tv-fcase')) { flowLayout(el, instant); swapBg(libAppOf(el.dataset.hero)); return; }
    keepInView(el, instant);
  }
  // The backdrop changes to another game's art: the new picture, once it's ready to draw, goes in
  // under the old one, which fades away over it. Moving on before it's ready skips it.
  function swapBg(a) {
    const bg = $('#tv-bg', root);
    if (!bg || bg.classList.contains('amb')) return;
    const src = a && (a.hero || a.cover) ? heroOf(a) : '';
    TV.bgWant = src;
    const cur = $('img:not(.gone), .tv-glow:not(.gone)', bg);
    if (cur && (cur.getAttribute('src') || '') === src) return;
    const put = () => {
      if (TV.bgWant !== src || !bg.isConnected) return;
      for (const o of $$('img:not(.gone), .tv-glow:not(.gone)', bg)) { o.classList.add('gone'); setTimeout(() => o.remove(), 380); }
      bg.insertAdjacentHTML('afterbegin', src ? `<img src="${esc(src)}" alt="" decoding="async">` : '<div class="tv-glow"></div>');
    };
    if (!src) return put();
    preload(src);
    const im = PRE.get(src);
    (im && im.decode ? im.decode().catch(() => {}) : Promise.resolve()).then(put);
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
      const segs = el.dataset.seg ? $$('.tv-seg > span', el) : [];
      html = `<span class="k">${esc(t('Setting'))}</span><h3>${esc(kEl ? kEl.firstChild.textContent : '')}</h3>${segs.length ? `<div class="val">${svg('chevL', 34)}<b>${esc(val)}</b>${svg('chevR', 34)}</div><div class="dots">${segs.map((x) => `<i class="${x.classList.contains('on') ? 'on' : ''}"></i>`).join('')}</div>` : val ? `<div class="val solo"><b>${esc(val)}</b></div>` : ''}<p>${esc(($('small', el) || {}).textContent || '')}</p>`;
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
    const down = $('.tv-down', root);
    if (down) down.style.opacity = y > 60 ? '0' : '';
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
    if (el.closest('.tv-sheet, .tv-viewer')) return;
    // The bar at the top (Back, the tabs): the page goes back to its top too.
    if (el.closest('.tv-bar')) { if (TV.scrollY) setScroll(0, instant); return; }
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
    } else {
      const pad = 70;
      if (top - y < pad) y = top - pad;
      if (top + h - y > H - pad) y = top + h - H + pad;
      // Anything that fits on the first screen shows with the page's top (its title, its logo).
      if (top < 260 || top + h < H - pad) y = 0;
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

  function moveTabPill(slide) {
    const p = $('.tv-tab-pill', stage()), on = $('.tv-tab.on', stage());
    if (!p) return;
    if (!on) { p.style.opacity = '0'; TV.tabPill = null; return; }
    const to = { x: on.offsetLeft, w: on.offsetWidth };
    const from = TV.tabPill;
    p.style.transition = 'none';
    if (from && slide && !still()) { p.style.transform = `translateX(${from.x}px)`; p.style.width = from.w + 'px'; void p.offsetWidth; p.style.transition = ''; }
    p.style.transform = `translateX(${to.x}px)`;
    p.style.width = to.w + 'px';
    TV.tabPill = to;
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
      // How far apart sideways: nothing when they overlap (a wide panel right above or below
      // counts as straight ahead), then a little for how far their middles are apart.
      const gapX = Math.max(0, Math.max(q.left, r.left) - Math.min(q.right, r.right)), gapY = Math.max(0, Math.max(q.top, r.top) - Math.min(q.bottom, r.bottom));
      if (dir === 'right') { if (q.left < r.right - 4) continue; main = q.left - r.right + Math.abs(dx) * 0.1; side = gapY + Math.abs(dy) * 0.05; }
      else if (dir === 'left') { if (q.right > r.left + 4) continue; main = r.left - q.right + Math.abs(dx) * 0.1; side = gapY + Math.abs(dy) * 0.05; }
      else if (dir === 'down') { if (q.top < r.bottom - 4) continue; main = q.top - r.bottom + Math.abs(dy) * 0.1; side = gapX + Math.abs(dx) * 0.05; }
      else { if (q.bottom > r.top + 4) continue; main = r.top - q.bottom + Math.abs(dy) * 0.1; side = gapX + Math.abs(dx) * 0.05; }
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
    TV.back.push({ view: TV.view, arg: TV.arg, key: (opts.fly && opts.fly.key) || (focused() || {}).dataset?.key, y: TV.scrollY, fly: opts.fly || null });
    if (GAMEVIEWS.includes(view)) TV.from = GAMEVIEWS.includes(TV.view) ? TV.from : TV.view;
    TV.view = view; TV.arg = arg;
    TV.enter = opts.enter || (view === 'game' ? 'game' : 'in');
    TV.scrollY = 0;
    if (!opts.quiet) sfx('open');
    draw();
  }

  function back() {
    if (TV.sheet) { if (TV.sheet.back && TV.sheet.back()) return; return closeSheet(); }
    // Discover narrowed to a tag: B takes the tag off first.
    if (TV.view === 'discover' && TV.dtag) { sfx('back'); TV.dtag = ''; TV.focusKey = 'g:@filter'; return discoverRedraw(); }
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
    TV.enter = b.fly && !calm() ? 'thruback' : 'back';
    draw();
    if (b.y) { setScroll(b.y, true); const f = focused(); if (f) keepInView(f, true); }
    if (b.fly) requestAnimationFrame(() => flyBack(b.fly));
  }

  function tab(step) {
    const order = tvTabs().map((x) => x[0]);
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
    // Home's dock: A plays (or installs, updates, sets up); the game's page is Y.
    if (el.dataset.dock) { const m = mainItem(libAppOf(el.dataset.dock)); if (m) { sfx('select'); return m.run(); } return fly(el, el.dataset.dock); }
    if (el.dataset.seg) return segStep(el.dataset.tvact, 1);
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
      case 'setupreq': return tvSetupReq(id);
      case 'addlib': sfx('select'); await addToLibrary(id); TV.focusKey = 'g-main'; return draw(true);
      case 'dsearch': return tvSearch();
      case 'signin': sfx('select'); return window.signIn && window.signIn(false);
      case 'tag': sfx('select'); TV.dtag = el.dataset.tag; TV.genre = ''; TV.dq = ''; TV.dorigin = ''; TV.gridShown = {}; return go('discover');
      case 'dtagclear': sfx('back'); TV.dtag = ''; TV.focusKey = 'g:@filter'; return discoverRedraw();
      case 'dfilter': return tvFilter();
      case 'gaction': {
        const run = async () => { try { const r = await invoke('game_action', { id, action: el.dataset.k }); if (r) toast(t(r)); } catch (e) { fail(e); } };
        const a = ((S.setup[id] || {}).actions || []).find((x) => x.id === el.dataset.k);
        if (!a || a.kind !== 'clear') return run();
        return sheet(tf('Clear its {what}?', { what: a.label.toLowerCase() }), [
          { label: t('Keep it'), sub: t('Nothing changes'), icon: 'chevL', run: () => {} },
          { label: t('Clear it'), sub: a.help, icon: 'bin', run },
        ]);
      }
      case 'more': sfx('open'); return gameSheet(id);
      case 'about': sfx('open'); return go('about');
      case 'support': sfx('select'); return invoke('open_url', { url: 'https://ko-fi.com/recomp' }).catch(fail);
      case 'storage': sfx('open'); TV.sizes = null; return go('storage');
      case 'saves': sfx('open'); TV.savesAll = null; return go('saves');
      case 'gsaves': return go('gsaves', id);
      case 'stor': {
        const g = S.library[id];
        if (!g) return;
        sfx('open');
        return sheet(g.name, [
          ...(!S.running.has(id) && !S.jobs[id] ? [{ label: t('Uninstall'), sub: t('Take it off this PC'), icon: 'bin', cls: 'danger', run: () => tvUninstall(id) }] : []),
          { label: t('Open its folder'), sub: g.path, icon: 'folder', run: () => invoke('open_folder', { id }).catch(fail) },
          { label: t('Game page'), icon: 'page', run: () => go('game', id) },
        ]);
      }
      case 'gs-cloud': {
        sfx('select');
        const g = S.library[id] || {};
        try { S.library[id] = await invoke('saves_cloud', { id, on: !g.cloud }); } catch (e) { fail(e); }
        return loadGameSaves(id);
      }
      case 'gs-backup': {
        sfx('select');
        try { const c = await invoke('saves_backup', { id }); toast(c ? t('Backed up.') : t('Nothing changed since the last copy.')); } catch (e) { fail(e); }
        return loadGameSaves(id);
      }
      case 'gs-open': return invoke('saves_open', { id }).catch(fail);
      case 'gs-restore': {
        sfx('open');
        const name = el.dataset.name || null, v = el.dataset.v || null;
        return sheet(t('Put this save back?'), [
          { label: t('Keep what\'s there'), sub: t('Nothing changes'), icon: 'chevL', run: () => {} },
          { label: t('Put it back'), sub: t('It takes the place of the game\'s saves on this PC'), icon: 'history', run: async () => { try { await invoke('saves_restore', { id, name, v }); toast(t('That save is back in place.')); } catch (e) { fail(e); } loadGameSaves(id); } },
        ]);
      }
    }
    if (el.dataset.goGame) {
      // A tile grows into the page; buttons and list items just go there.
      if (el.matches('.tv-fcase, .tv-poster, .tv-card, .tv-dk')) return fly(el, el.dataset.goGame);
      return go('game', el.dataset.goGame);
    }
  }

  // Settings with a few choices side by side: A or right goes to the next, left to the one before.
  async function segStep(act, dir) {
    const roll = (list, cur) => list[(list.indexOf(cur) + dir + list.length) % list.length];
    const ui = S.settings.ui || {};
    sfx('select');
    if (act === 'size') { await saveSettings({ tv_size: roll(['', 'small', 'medium', 'large'], S.settings.tv_size || '') }); TV.pill = null; TV.tabPill = null; }
    else if (act === 'libview') await saveUi({ tv_view: roll(['', 'flow'], ui.tv_view || '') });
    else if (act === 'homeview') await saveUi({ tv_home: roll(['', 'pinned'], ui.tv_home || '') });
    else if (act === 'dockview') await saveUi({ tv_dock: roll(['', 'wide'], ui.tv_dock || '') });
    else if (act === 'bgmode') await saveUi({ tv_bg: roll(['', 'grid', 'drift', 'art'], ui.tv_bg || '') });
    else if (act === 'glyphs') await saveUi({ tv_glyphs: roll(['', 'xbox', 'ps', 'nintendo'], ui.tv_glyphs || '') });
    else if (act === 'theme') {
      const now = ui.theme === 'light' ? 2 : ui.oled ? 1 : 0, next = (now + dir + 3) % 3;
      await saveUi({ theme: next === 2 ? 'light' : '', oled: next === 1 });
      if (window.renderSide) window.renderSide();
      setTone(null);
    }
    TV.panelKey = null;
    return draw(true);
  }

  function press(name) {
    if (!TV.on) return false;
    // While the camera is flying into or out of a game, it finishes first.
    if (TV.flying) return true;
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
    if (TV.sort && !TV.sheet) {
      if (name === 'left' || name === 'right') sortMove(name === 'right' ? 1 : -1);
      else if (name === 'a' || name === 'x') endSort(true);
      else if (name === 'b') endSort(false);
      return true;
    }
    switch (name) {
      case 'left': case 'right': if (cur && cur.dataset.gset) { cycleSetting(cur, name === 'right' ? 1 : -1); break; }
        if (cur && cur.dataset.seg && !TV.sheet) { segStep(cur.dataset.tvact, name === 'right' ? 1 : -1); break; }
      // falls through
      case 'up': case 'down': { const before = focused(); move(name); if (focused() !== before) sfx('move'); break; }
      case 'a': activate(cur); break;
      case 'b': if (TV.sheet || TV.back.length || TV.view !== 'home') sfx('back'); back(); break;
      case 'x': if (TV.view === 'mods') { const mm = S.mods[TV.arg]; if (mm && mm.community && mm.gamebanana) { S.modSrc[TV.arg] = (S.modSrc[TV.arg] || 'ts') === 'ts' ? 'gb' : 'ts'; sfx('move'); draw(true); } break; }
        if (TV.view === 'shots' && cur && cur.dataset.shotI !== undefined) { TV.shot = Number(cur.dataset.shotI); shotAct('x'); break; }
        if (TV.sheet && (TV.sheet.vers || TV.sheet.game)) { sfx('back'); closeSheet(); break; }
        if (!TV.sheet) {
          // Options for the game in focus, or the page's game.
          const gid = (cur && (cur.dataset.hero || cur.dataset.goGame || (cur.dataset.tvact === 'stor' && cur.dataset.id))) || (GAMEVIEWS.includes(TV.view) && TV.arg);
          if (gid) { sfx('open'); gameSheet(gid); }
        }
        break;
      case 'y': {
        if (TV.sheet) break;
        const id = cur && (cur.dataset.hero || cur.dataset.goGame || (cur.closest('.tv-hero') && TV.hero));
        if (id && TV.view !== 'game') {
          // From Home's big picture, the camera flies into that game's cover in the row below.
          const from = cur.matches('.tv-dk, .tv-poster, .tv-fcase, .tv-card') ? cur : $(`.tv-dk[data-dock="${CSS.escape(id)}"]`, stage());
          if (from) fly(from, id); else go('game', id);
        }
        break;
      }
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
  // Only presses sent to this window (the quick menu over a game gets its own).
  ((w) => (w && typeof w.listen === 'function' ? w.listen.bind(w) : T.event.listen))(T.webviewWindow && T.webviewWindow.getCurrentWebviewWindow && T.webviewWindow.getCurrentWebviewWindow())('pad', ({ payload }) => {
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
    setTimeout(pollPads, document.hasFocus() && !S.away ? (TV.on ? 16 : 33) : 500);
    if (!document.hasFocus() || S.away) return;
    for (const gp of navigator.getGamepads()) {
      if (!gp) continue;
      const kind = /054c|sony|dualsense|playstation|wireless controller/i.test(gp.id) ? 'ps' : /057e|nintendo|pro controller|joy-con/i.test(gp.id) ? 'nintendo' : 'xbox';
      const now = {};
      // Buttons go by what's printed on them: on a Nintendo pad A is on the right, B at the bottom.
      const nin = kind === 'nintendo' ? { 0: 'b', 1: 'a', 2: 'y', 3: 'x' } : {};
      gp.buttons.forEach((bt, i) => { const k = nin[i] || STD[i]; if (k && bt.pressed) now[k] = 1; });
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
    // The search sheet takes letters, numbers and Backspace from a keyboard.
    const kb = TV.sheet && TV.sheet.kb;
    if (kb && !e.ctrlKey && !e.altKey && !e.metaKey && (/^[a-z0-9 '&:.\-]$/i.test(e.key) || e.key === 'Backspace')) {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === 'Backspace') kb.del(); else kb.type(e.key.toLowerCase());
      return;
    }
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
    const fk = e.target.closest('.tv-fake[data-hact]');
    if (fk) { e.stopPropagation(); const id = fk.dataset.id; if (fk.dataset.hact === 'page') { const d = $(`.tv-dk[data-dock="${CSS.escape(id)}"]`, stage()); return d ? fly(d, id) : go('game', id); } const m = mainItem(libAppOf(id)); return m ? m.run() : go('game', id); }
    const el = e.target.closest('[data-f]');
    if (!el) return;
    if (el.matches('.tv-fcase') && !el.classList.contains('f')) { e.stopPropagation(); return focus(el); }
    e.stopPropagation();
    if (!el.classList.contains('f')) focus(el);
    activate(el);
  });

  // Keep the pages current while things happen elsewhere.
  let redraw = 0;
  const soon = () => { if (!TV.on || TV.booting || $('.tv-boot', root)) return; clearTimeout(redraw); redraw = setTimeout(() => draw(true), 120); };
  T.event.listen('install', ({ payload: p }) => { if (['done', 'error', 'cancelled', 'prepare', 'queued', 'unpack', 'clone', 'verify', 'backup'].includes(p.phase) || (p.phase === 'download' && !TV.dl[p.id])) { TV.dl[p.id] = p.phase === 'download'; setTimeout(soon, 300); } });
  // A logo that doesn't load becomes the name, sized to fit.
  root.addEventListener('error', (e) => { if (e.target && e.target.classList && e.target.classList.contains('tv-logo')) setTimeout(fitAll, 0); }, true);
  T.event.listen('game', () => setTimeout(soon, 300));
  T.event.listen('screenshot', () => setTimeout(soon, 400));
  setInterval(() => { const c = $('#tv-clock', root); if (c) c.textContent = clock(); }, 20000);
})();
