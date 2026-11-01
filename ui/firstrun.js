// The first time the launcher opens: a short setup, like a new console. Language, how you play,
// where your games are (and which have recomps), where games go, a few choices. Mouse, keyboard
// or controller.
(() => {
  'use strict';
  const STEPS = ['lang', 'welcome', 'play', 'games', 'library', 'choices', 'data', 'account', 'done'];
  const O = { el: null, step: 0, scanning: false, scanned: false, dir: 1 };
  const LOGO = '<svg viewBox="0 0 256 256" aria-hidden="true"><defs><linearGradient id="oobg" x1="0" y1="38" x2="0" y2="218" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="#34f2e8"/><stop offset="1" stop-color="#377bb6"/></linearGradient></defs><g fill="url(#oobg)" transform="translate(4 -2)"><path d="M40 40H88V168H116L64 220L12 168H40Z"/><path d="M104 40H172L216 84V116L180 152H104V116H164L180 100V92L164 76H104Z"/><path d="M132 168H184L236 220H184Z"/></g></svg>';
  const ICON = {
    desk: '<svg width="46" height="46" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="4" width="18" height="12" rx="2"/><path d="M8 20h8M12 16v4"/></svg>',
    tv: '<svg width="46" height="46" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="2" y="5" width="20" height="13" rx="2"/><path d="M8 21h8"/><path d="M9 10.5h2M10 9.5v2M14.5 10h.01M15.5 11h.01"/></svg>',
    find: '<svg width="26" height="26" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true"><circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/></svg>',
    disc: '<svg width="26" height="26" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><circle cx="12" cy="12" r="9"/><circle cx="12" cy="12" r="2.5"/></svg>',
    look: '<svg width="26" height="26" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round" aria-hidden="true"><path d="M12 3.5l1.9 5.1 5.1 1.9-5.1 1.9L12 17.5l-1.9-5.1L5 10.5l5.1-1.9z"/><path d="M18.5 15.5l.8 2 2 .8-2 .8-.8 2-.8-2-2-.8 2-.8z"/></svg>',
    folder: '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round" aria-hidden="true"><path d="M3 6.5A1.5 1.5 0 0 1 4.5 5H10l2 2.5h7.5A1.5 1.5 0 0 1 21 9v9.5a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 18.5z"/></svg>',
    user: '<svg width="46" height="46" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="8.5" r="3.6"/><path d="M4.8 20a7.2 7.2 0 0 1 14.4 0"/></svg>',
    plus: '<svg width="46" height="46" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="10" cy="8.5" r="3.4"/><path d="M3.5 20a6.6 6.6 0 0 1 11.6-4.3M18.5 14v6M15.5 17h6"/></svg>',
    cloud: '<svg width="64" height="64" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M7 18a4.5 4.5 0 0 1-.6-9 6 6 0 0 1 11.5 1.6A3.8 3.8 0 0 1 17.5 18z"/><path d="m9.5 13.5 2.5 2.5 3-3.5" /></svg>',
    check: '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m5 12 5 5 9-10"/></svg>',
  };
  const $o = (s) => O.el && O.el.querySelector(s);
  const $$o = (s) => (O.el ? [...O.el.querySelectorAll(s)] : []);

  function firstRun() {
    if (O.el) return;
    // Setup is always in a window, whatever was picked before (Big Picture comes after, if chosen).
    try { window.__TAURI__.window.getCurrentWindow().setFullscreen(false); } catch (e) { /* fine */ }
    O.el = document.createElement('div');
    O.el.className = 'oob';
    O.el.setAttribute('role', 'dialog');
    O.el.setAttribute('aria-modal', 'true');
    O.el.innerHTML = '<div class="oob-aurora" aria-hidden="true"><i></i><i></i><i></i></div><div class="oob-top" data-tauri-drag-region><img class="oob-logo-h" src="img/logo.svg" alt="RecompLauncher"><div class="win oob-win"><button type="button" data-o="w-min" aria-label="Minimize"><svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><path d="M1.5 6h9" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/></svg></button><button type="button" data-o="w-max" aria-label="Maximize"><svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><rect x="1.5" y="1.5" width="9" height="9" rx="2" fill="none" stroke="currentColor" stroke-width="1.3"/></svg></button><button type="button" class="close" data-o="w-close" aria-label="Close"><svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><path d="m2 2 8 8m0-8-8 8" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/></svg></button></div></div><div class="oob-stage"></div><div class="oob-foot"></div>';
    document.body.append(O.el);
    O.el.addEventListener('click', onClick);
    O.el.addEventListener('keydown', onKey);
    O.step = 0;
    // A Steam Deck is mostly played with its controls: Big Picture mode is the way it starts.
    if (S.deck && !(S.settings.ui || {}).deck_seen) save({ start_in_tv: true, ui: Object.assign({}, S.settings.ui || {}, { deck_seen: true }) }).then(draw);
    draw();
  }
  window.firstRun = firstRun;

  const games = () => Object.keys(S.owned || {}).map((id) => byId(id) || { id, name: id }).filter(Boolean);
  const sw = (k, label, sub) => `<div class="oob-row"><div class="k"><b>${esc(label)}</b><span>${esc(sub)}</span></div><button type="button" class="switch" role="switch" aria-checked="${S.settings[k] !== false}" aria-label="${esc(label)}" data-o="sw" data-k="${k}"></button></div>`;
  const covers = (list) => `<div class="oob-covers">${list.slice(0, 6).map((a) => `<div class="oob-cover" title="${esc(a.name)}">${a.cover ? pic(a.cover) : `<span>${esc(a.name)}</span>`}</div>`).join('')}</div>`;

  function body(k) {
    switch (k) {
      case 'lang': {
        // The computer's own language first, then the 23 others: a full grid.
        const chosen = (S.settings.ui || {}).lang || 'auto';
        const sys = LANGS.find((l) => l[0] === systemLang());
        return `<div class="oob-logo">${LOGO}</div><h1>${esc(t('Choose your language'))}</h1>
          <div class="oob-langs" role="radiogroup" aria-label="${esc(t('Language'))}"><button type="button" role="radio" aria-checked="${chosen === 'auto'}" class="oob-lang auto" data-o="lang" data-c="auto"><span>${esc(S.os === 'windows' ? t('Same as Windows') : t('Same as your computer'))}</span><small>${esc(sys ? sys[1] : 'English (US)')}</small></button>${LANGS.map(([c, n]) => `<button type="button" role="radio" aria-checked="${c === chosen}" class="oob-lang" data-o="lang" data-c="${c}" lang="${c}">${esc(n)}</button>`).join('')}</div>`;
      }
      case 'welcome':
        return `<div class="oob-logo big">${LOGO}</div><h1>${esc(t('Welcome to RecompLauncher'))}</h1><p class="oob-lead">${esc(t('Your own games, rebuilt for PC. A minute of setup and you\'re in.'))}</p>
          <div class="oob-three">
            <div class="oob-card">${ICON.find}<b>${esc(t('Finds your games'))}</b><span>${esc(t('Point it at your game backups and it spots the ones with recomps.'))}</span></div>
            <div class="oob-card">${ICON.disc}<b>${esc(t('Sets them up from your disc'))}</b><span>${esc(t('Your disc image is checked once. After that, you just press Play.'))}</span></div>
            <div class="oob-card">${ICON.look}<b>${esc(t('Your way'))}</b><span>${esc(t('Settings, looks and mods for every game, at a desk or on a TV.'))}</span></div>
          </div>`;
      case 'play': {
        const tv = !!S.settings.start_in_tv;
        return `<h1>${esc(t('How will you play?'))}</h1><p class="oob-lead">${esc(t('You can switch any time.'))}</p>
          <div class="oob-two" role="radiogroup" aria-label="${esc(t('How will you play?'))}">
            <button type="button" role="radio" aria-checked="${!tv}" class="oob-pick" data-o="play" data-v="desk">${ICON.desk}<b>${esc(t('At a desk'))}</b><span>${esc(t('A window, with a mouse and keyboard. Hold Menu on a controller to switch to Big Picture mode.'))}</span></button>
            <button type="button" role="radio" aria-checked="${tv}" class="oob-pick" data-o="play" data-v="tv">${ICON.tv}<b>${esc(t('On a TV'))}</b><span>${esc(t('Full screen and made for a controller. It opens this way every time.'))}</span></button>
          </div>${S.deck && S.steam ? `<div class="oob-deck"><div><b>${esc(t('On a Steam Deck'))}</b><span>${esc((S.settings.ui || {}).in_steam ? t('RecompLauncher is in your Steam library. Open it from Gaming Mode.') : t('Add RecompLauncher to Steam and it\'s right there in Gaming Mode, opening in Big Picture mode.'))}</span></div>${(S.settings.ui || {}).in_steam ? '' : `<button type="button" class="btn main" data-o="deck-steam">${esc(t('Add to Steam'))}</button>`}</div>` : ''}`;
      }
      case 'games': {
        const dirs = S.settings.rom_dirs || [];
        const found = games();
        const status = O.scanning ? `<p class="oob-status"><span class="spinr"></span>${esc(t('Looking through your folders…'))}</p>`
          : found.length ? `<p class="oob-status ok">${ICON.check}${esc(tn(found.length, 'Found 1 game that has a recomp', 'Found {n} games that have recomps'))}</p>${covers(found)}`
            : O.scanned && dirs.length ? `<p class="oob-status">${esc(t('None of these have recomps yet. You\'ll hear about it when one does.'))}</p>` : '';
        return `<h1>${esc(t('Where do you keep your games?'))}</h1><p class="oob-lead">${esc(t('Add the folders with your disc images, ROMs and game backups. The launcher finds the games that have recomps. Nothing leaves your PC.'))}</p>
          <div class="oob-box">${dirs.map((d) => `<div class="oob-dir">${ICON.folder}<span title="${esc(d)}">${esc(d)}</span><button type="button" class="btn sm" data-o="rm" data-path="${esc(d)}">${esc(t('Remove'))}</button></div>`).join('')}
          <button type="button" class="btn${dirs.length ? '' : ' main'} oob-add" data-o="add">${ICON.folder}${esc(dirs.length ? t('Add another folder') : t('Add a folder'))}</button></div>${status}`;
      }
      case 'library': {
        const lib = S.settings.library_dir || '';
        const info = (S.libs || []).find((l) => l.path === lib);
        return `<h1>${esc(t('Where should games go?'))}</h1><p class="oob-lead">${esc(t('Recomps install here. You can add more places, on other drives, in Settings.'))}</p>
          <div class="oob-box"><div class="oob-dir big">${ICON.folder}<span title="${esc(lib)}">${esc(lib)}</span><button type="button" class="btn sm" data-o="lib">${esc(t('Change'))}</button></div>
          ${info && info.free ? `<p class="oob-free">${esc(tf('{free} free on this drive', { free: fmtB(info.free) }))}</p>` : ''}</div>
          <div class="oob-deck"><div><b>${esc(t('Already have recomps?'))}</b><span>${esc(t('Bring in the ones you installed yourself, with Quiver Launcher or in Steam. They keep updating from here, where they are.'))}</span></div><button type="button" class="btn" data-o="adopt">${esc(t('Bring Your Recomps'))}</button></div>`;
      }
      case 'choices':
        return `<h1>${esc(t('A few choices'))}</h1><p class="oob-lead">${esc(t('All of these are in Settings later.'))}</p>
          <div class="oob-box rows">
            ${sw('check_updates', t('Look for updates'), t('Checks for new versions of your games when the launcher opens.'))}
            ${sw('backup_before_update', t('Back up your saves'), t('Before every update, so you can always go back.'))}
            ${sw('notify', t('Notifications'), t('When updates are ready, and when a game you own gets a recomp.'))}
            ${sw('discord', t('Discord'), t('Shows the game you\'re playing on your profile.'))}
          </div>`;
      case 'data': {
        // Off unless the player turns them on.
        const sw2 = (k, label, sub) => `<div class="oob-row"><div class="k"><b>${esc(label)}</b><span>${esc(sub)}</span></div><button type="button" class="switch" role="switch" aria-checked="${!!S.settings[k]}" aria-label="${esc(label)}" data-o="sw" data-k="${k}"></button></div>`;
        return `<h1>${esc(t('Help Improve RecompLauncher?'))}</h1><p class="oob-lead">${esc(t('Both are off unless you turn them on. You can change this any time in Settings.'))}</p>
          <div class="oob-box rows">
            ${sw2('share_usage', t('Share usage and diagnostics'), t('Your PC\'s parts, which games you play and for how long, and what went wrong when one crashes or won\'t install. No names, no files.'))}
            ${sw2('share_stats', t('Share anonymous usage stats'), t('When the launcher is open and which recomps are being played, under a random ID that isn\'t linked to you or your account.'))}
          </div><p class="oob-free"><a href="#" data-o="what">${esc(t('What\'s sent'))}</a></p>`;
      }
      case 'account': {
        const a = S.account || {};
        if (a.signedIn) {
          return `<div class="oob-logo oob-cloud">${ICON.cloud}</div><h1>${esc(t('You\'re signed in'))}</h1><p class="oob-lead">${esc(tf('As {name}. Each game asks before you first play it whether its saves go to your account.', { name: a.user.username }))}</p>`;
        }
        return `<div class="oob-logo oob-cloud">${ICON.cloud}</div><h1>${esc(t('Keep your saves safe'))}</h1><p class="oob-lead">${esc(t('With a free RecompStore account, your saves go to the cloud after you play, ready on any PC. Everything works without one too.'))}</p>
          <div class="oob-two">
            <button type="button" class="oob-pick" data-o="acc" data-v="in">${ICON.user}<b>${esc(t('Sign in'))}</b><span>${esc(t('With Google, Discord, GitHub or your email, in your browser.'))}</span></button>
            <button type="button" class="oob-pick" data-o="acc" data-v="new">${ICON.plus}<b>${esc(t('Create an account'))}</b><span>${esc(t('Takes a minute in your browser. It\'s the same account as recompstore.com.'))}</span></button>
          </div>`;
      }
      default: {
        const found = games();
        return `<div class="oob-logo big done">${LOGO}</div><h1>${esc(t('You\'re all set'))}</h1>
          <p class="oob-lead">${esc(found.length ? tn(found.length, 'One of your games has a recomp. It\'s in your Library, ready to install.', '{n} of your games have recomps. They\'re in your Library, ready to install.') : t('Find something to play in Discover.'))}</p>${found.length ? covers(found) : ''}`;
      }
    }
  }

  function draw() {
    if (!O.el) return;
    const k = STEPS[O.step];
    const stage = $o('.oob-stage');
    stage.innerHTML = `<div class="oob-step ${O.dir < 0 ? 'back' : ''}" data-step="${k}">${body(k)}</div>`;
    const last = O.step === STEPS.length - 1;
    $o('.oob-foot').innerHTML = `<div class="oob-side">${O.step > 0 ? `<button type="button" class="btn" data-o="back">${esc(t('Back'))}</button>` : ''}</div>
      <div class="oob-dots" aria-hidden="true">${STEPS.map((s, i) => `<i class="${i === O.step ? 'on' : i < O.step ? 'past' : ''}"></i>`).join('')}</div>
      <div class="oob-side r">${k === 'games' && !(S.settings.rom_dirs || []).length ? `<button type="button" class="btn" data-o="next">${esc(t('Skip'))}</button>` : ''}<button type="button" class="btn main" data-o="${last ? 'finish' : 'next'}"${O.scanning ? ' disabled' : ''}>${esc(last ? t('Start playing') : k === 'account' && !(S.account && S.account.signedIn) ? t('No thanks') : t('Continue'))}</button></div>`;
    O.el.setAttribute('aria-label', t('Set up RecompLauncher'));
    const f = $o('.oob-step [aria-checked="true"]') || $o('.oob-step .btn.main') || $o('.oob-foot .btn.main');
    if (f) f.focus({ preventScroll: true });
  }

  async function save(patch) {
    const next = Object.assign({}, S.settings, patch);
    try { await invoke('save_settings', { settings: next }); S.settings = next; } catch (e) { fail(e); }
  }

  async function scan() {
    O.scanning = true; draw();
    try { S.roms = await invoke('scan_roms'); S.owned = await invoke('rom_matches'); if (window.addOwned) await window.addOwned(); } catch (e) { fail(e); }
    O.scanning = false; O.scanned = true;
    draw();
    render(true);
  }

  async function onClick(e) {
    const b = e.target.closest('[data-o]');
    if (!b || b.disabled) return;
    const win = () => window.__TAURI__.window.getCurrentWindow();
    switch (b.dataset.o) {
      // Setup is in a window like the rest: it can be moved, made smaller or closed.
      case 'w-min': return win().minimize();
      case 'w-max': return win().toggleMaximize();
      case 'w-close': return win().close();
      case 'lang': {
        const c = b.dataset.c;
        await loadLang(c === 'auto' ? systemLang() : c);
        await save({ ui: Object.assign({}, S.settings.ui || {}, { lang: c }) });
        tDom();
        draw();
        const again = $o(`[data-c="${c}"]`);
        if (again) again.focus({ preventScroll: true });
        render(true);
        return;
      }
      case 'deck-steam': {
        b.disabled = true;
        if (window.selfToSteam) await window.selfToSteam(false);
        b.disabled = false;
        return draw();
      }
      case 'play':
        await save({ start_in_tv: b.dataset.v === 'tv' });
        $$o('.oob-pick').forEach((x) => x.setAttribute('aria-checked', String(x === b)));
        return;
      case 'add': {
        const p = await pickFile({ title: t('A folder with your games'), directory: true }).catch(fail);
        if (!p) return;
        await save({ rom_dirs: uniq([...(S.settings.rom_dirs || []), p]) });
        return scan();
      }
      case 'rm':
        await save({ rom_dirs: (S.settings.rom_dirs || []).filter((d) => d !== b.dataset.path) });
        return scan();
      case 'lib': {
        const p = await pickFile({ title: t('Where new games install'), directory: true, defaultPath: S.settings.library_dir }).catch(fail);
        if (!p) return;
        await save({ library_dir: p, asked_location: true });
        try { S.libs = await invoke('libraries'); } catch (err) { /* fine */ }
        return draw();
      }
      case 'sw': {
        const k = b.dataset.k, on = b.getAttribute('aria-checked') !== 'true';
        b.setAttribute('aria-checked', String(on));
        return save({ [k]: on });
      }
      case 'acc': return window.signIn && window.signIn(b.dataset.v === 'new');
      case 'adopt': return window.adoptAct && window.adoptAct('adopt', b);
      case 'what': e.preventDefault(); return invoke('open_url', { url: 'https://recompstore.com/privacy#launcher' }).catch(fail);
      case 'back': return go(-1);
      case 'next': return go(1);
      case 'finish': return finish();
    }
  }

  function go(d) {
    const n = O.step + d;
    if (n < 0 || n >= STEPS.length) return;
    O.dir = d;
    O.step = n;
    if (STEPS[n] === 'library') invoke('libraries').then((l) => { S.libs = l; if (STEPS[O.step] === 'library') draw(); }).catch(() => {});
    draw();
  }

  async function finish() {
    if (O.finishing) return;
    O.finishing = true;
    await save({ setup_done: true, asked_location: true, data_asked: true });
    if (!O.el) return;
    O.el.classList.add('out');
    const tv = !!S.settings.start_in_tv;
    setTimeout(() => { if (O.el) O.el.remove(); O.el = null; }, 450);
    S.view = 'home';
    S.anim = 'enter';
    render(true);
    if (tv && window.startTV) window.startTV();
  }

  // Arrow keys and a controller's d-pad move between the things on screen.
  function nearest(dir) {
    const cur = document.activeElement && O.el.contains(document.activeElement) ? document.activeElement : null;
    const all = $$o('button:not([disabled])').filter((x) => x.offsetParent);
    if (!cur) { if (all[0]) all[0].focus(); return; }
    const r = cur.getBoundingClientRect(), cx = r.left + r.width / 2, cy = r.top + r.height / 2;
    let best = null, score = Infinity;
    for (const e of all) {
      if (e === cur) continue;
      const q = e.getBoundingClientRect(), ex = q.left + q.width / 2, ey = q.top + q.height / 2;
      const dx = ex - cx, dy = ey - cy;
      let main, side;
      if (dir === 'right') { if (q.left < r.right - 4) continue; main = dx; side = dy; }
      else if (dir === 'left') { if (q.right > r.left + 4) continue; main = -dx; side = dy; }
      else if (dir === 'down') { if (q.top < r.bottom - 4) continue; main = dy; side = dx; }
      else { if (q.bottom > r.top + 4) continue; main = -dy; side = dx; }
      const sc = main + Math.abs(side) * 2.4;
      if (sc < score) { score = sc; best = e; }
    }
    if (best) best.focus();
  }
  function onKey(e) {
    if (e.target.matches('input')) return;
    const dirs = { ArrowUp: 'up', ArrowDown: 'down', ArrowLeft: 'left', ArrowRight: 'right' };
    if (dirs[e.key]) { e.preventDefault(); nearest(dirs[e.key]); }
    else if (e.key === 'Escape' && O.step > 0) { e.preventDefault(); go(-1); }
  }
  // Signed in while setup is open: the Account step shows it.
  window.oobRedraw = () => { if (O.el && STEPS[O.step] === 'account') draw(); };
  window.oobPad = (b) => {
    if (!O.el || document.querySelector('.scrim')) return false;
    if (['up', 'down', 'left', 'right'].includes(b)) nearest(b);
    else if (b === 'a') { const f = document.activeElement; if (f && O.el.contains(f)) f.click(); }
    else if (b === 'b') go(-1);
    else if (b === 'menu') { const n = $o('.oob-foot .btn.main'); if (n) n.click(); }
    return true;
  };
})();
