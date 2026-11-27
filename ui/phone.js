// The phone app: the Android launcher without a controller. An app made for fingers, upright or
// sideways: tabs that swipe, a floating menu, a game's page that its cover flies into, sheets that
// pull down. With a controller it's Big Picture mode (tv.js): pressing a button on a controller
// switches to it, and when the last controller goes, it comes back here.
//
// How it's drawn: the frame (tabs, menu, layers) is built once. Each tab keeps its own page and
// its place; a change (a download ticking, a game installed) patches the pages in place with
// morph(), so covers stay, nothing blinks and nothing jumps under the finger. Everything is laid
// out at one design size and scaled to the screen (see fit()).
(function () {
  const root = document.getElementById('phone');
  if (!root) return;

  // ---------- what the player chose (kept with the settings) ----------
  const TABS = { home: N_('Home'), catalog: N_('Catalog'), library: N_('Library'), downloads: N_('Downloads'), saves: N_('Saves'), soon: N_('Coming Soon'), settings: N_('Settings') };
  const TAB_SHORT = { soon: N_('Soon') };
  const TABS_FIRST = ['home', 'catalog', 'library', 'saves', 'settings'];
  // Home and Settings can't be taken off the menu: one is the way in, the other the way to put tabs back.
  const TABS_FIXED = ['home', 'settings'];
  const SIZES = { s: 0.82 / 0.9, m: 1, l: 1 / 0.9 };
  const THEMES = ['dark', 'oled', 'light'];
  const ui = () => (S.settings && S.settings.ui) || {};
  const pref = {
    tabs: () => { const t0 = (ui().phone_tabs || []).filter((k) => TABS[k]); return t0.length >= 3 ? Object.keys(TABS).filter((k) => t0.includes(k)) : TABS_FIRST; },
    labels: () => ui().phone_labels === true,
    haptics: () => ui().phone_haptics !== false,
    size: () => (SIZES[ui().phone_size] ? ui().phone_size : 'm'),
    theme: () => (ui().theme === 'light' ? 'light' : ui().oled ? 'oled' : 'dark'),
    libview: () => (ui().phone_libview === 'list' ? 'list' : 'grid'),
    padTv: () => ui().pad_tv !== false,
  };

  // P: where the player is. stack: game pages open (the last shows). subs: pages inside a tab.
  const P = { on: false, booted: false, tab: 'home', libi: 0, stack: [], subs: [], search: false, src: null, land: false, z: 1 };
  let el = {};

  // ---------- small pieces ----------
  const ICON = {
    home: '<path d="M3 10.5 12 3l9 7.5V20a1 1 0 0 1-1 1h-5v-6H9v6H4a1 1 0 0 1-1-1z"/>',
    catalog: '<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/>',
    library: '<rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/>',
    settings: '<path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/><circle cx="12" cy="12" r="3"/>',
    downloads: '<path d="M12 3v12m0 0-5-5m5 5 5-5M4 21h16"/>',
    soon: '<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>',
    play: '<path d="M7 4.5v15l13-7.5z" fill="currentColor" stroke="none"/>',
    back: '<path d="M15 5l-7 7 7 7"/>',
    more: '<circle cx="5" cy="12" r="1.3" fill="currentColor"/><circle cx="12" cy="12" r="1.3" fill="currentColor"/><circle cx="19" cy="12" r="1.3" fill="currentColor"/>',
    search: '<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/>',
    x: '<path d="M6 6l12 12M18 6 6 18"/>',
    check: '<path d="m5 12.5 4.5 4.5L19 7.5"/>',
    bookmark: '<path d="M6 3h12v18l-6-4-6 4z"/>',
    trash: '<path d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3"/>',
    file: '<path d="M14 3H6v18h12V7zM14 3v4h4"/>',
    list: '<path d="M8 6h13M8 12h13M8 18h13M3.5 6h.01M3.5 12h.01M3.5 18h.01"/>',
    tv: '<rect x="2" y="5" width="20" height="13" rx="2.5"/><path d="M8 21h8"/>',
    globe: '<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18"/>',
    user: '<circle cx="12" cy="8" r="4"/><path d="M4 21a8 8 0 0 1 16 0"/>',
    chev: '<path d="m9 5 7 7-7 7"/>',
    chat: '<path d="M21 12a8 8 0 0 1-11.6 7.1L4 20l1-4.6A8 8 0 1 1 21 12z"/>',
    saves: '<path d="M15.2 3a2 2 0 0 1 1.4.6l3.8 3.8a2 2 0 0 1 .6 1.4V19a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2zM17 21v-7a1 1 0 0 0-1-1H8a1 1 0 0 0-1 1v7M7 3v4a1 1 0 0 0 1 1h7"/>',
    cloud: '<path d="M7 18a4.5 4.5 0 0 1-.6-9 6 6 0 0 1 11.5 1.6A3.8 3.8 0 0 1 17.5 18z"/>',
    up: '<path d="M12 20V9m0 0-4.5 4.5M12 9l4.5 4.5M5 4h14"/>',
    phone: '<rect x="6" y="2.5" width="12" height="19" rx="2.5"/><path d="M11 18.5h2"/>',
    key: '<circle cx="8" cy="15" r="4"/><path d="m11 12 9-9m-4 4 3 3"/>',
  };
  const ic = (k, cls = '') => `<svg class="ph-ic ${cls}" viewBox="0 0 24 24" aria-hidden="true">${ICON[k]}</svg>`;
  const MARK = '<svg width="W" height="W" viewBox="0 0 256 256" role="img" aria-label="RecompLauncher"><defs><linearGradient id="ph-mg" x1="0" y1="38" x2="0" y2="218" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="#34f2e8"/><stop offset="1" stop-color="#377bb6"/></linearGradient></defs><g fill="url(#ph-mg)" transform="translate(4 -2)"><path d="M40 40H88V168H116L64 220L12 168H40Z"/><path d="M104 40H172L216 84V116L180 152H104V116H164L180 100V92L164 76H104Z"/><path d="M132 168H184L236 220H184Z"/></g></svg>';
  const mark = (w) => MARK.replace(/W/g, w);
  const RANK_COLORS = ['#f5c518', '#c0c8d4', '#d08a4e'];
  // Android and the web engine drawing this (Android System WebView), for telling what a phone has.
  const engine = () => { const u = navigator.userAgent, a = /Android ([\d.]+)/.exec(u), c = /Chrome\/(\d+)/.exec(u); return [a ? 'Android ' + a[1] : '', c ? tf('web engine {n}', { n: c[1] }) : ''].filter(Boolean).join(' · '); };
  // A light tick under the finger, when the player wants them.
  const buzz = (kind = 'tap') => { if (pref.haptics() && S.os === 'android') invoke('android_haptic', { kind }).catch(() => {}); };

  // A picture that has loaded before shows at once instead of fading in again.
  const SEEN = new Set();
  function img(url, cls = '', more = '') {
    if (!url) return '';
    const c = [cls, SEEN.has(url) ? 'ld' : ''].filter(Boolean).join(' ');
    return `<img src="${esc(url)}" alt=""${c ? ` class="${c}"` : ''} loading="lazy" decoding="async" draggable="false"${more ? ' ' + more : ''}>`;
  }
  const coverUrl = (a, w = 320) => (a && a.cover ? media(thumbOf(a.cover, w)) : '');
  const heroUrl = (a) => (a && (a.hero || a.cover) ? media(a.hero || a.cover) : '');
  const coverBox = (a, cls = '', id = '') => `<span class="ph-cover ${cls}"${id ? ` id="${id}"` : ''}><i>${esc(a.name)}</i>${img(coverUrl(a))}</span>`;

  // ---------- the games ----------
  const pool = () => visible().filter((a) => forOS(a) && !a.soon);
  const appOf = (id) => byId(id) || (S.library[id] ? libApp(S.library[id]) : null);
  const conName = (a) => originName(a.origin) || '';
  const conColor = (k) => CONSOLE_COLORS[k] || CONSOLE_COLORS.other;
  const conDot = (k, big) => `<i class="ph-dot${big ? ' b' : ''}" style="--c:${conColor(k)}"></i>`;
  const sizeOf = (a) => a.android_size || '';
  const devName = (a) => (devOf(a) || {}).name || a.author || '';
  const stateOf = (a) => status(a);
  const isBusy = (a) => stateOf(a) === 'busy';
  const jobOf = (id) => S.jobs[id] || (S.paused || {})[id];
  // The consoles that have games here, the fullest first: [{ k, name, list }].
  function consoles() {
    const by = new Map();
    for (const a of pool()) {
      if (!a.origin) continue;
      if (!by.has(a.origin)) by.set(a.origin, []);
      by.get(a.origin).push(a);
    }
    const out = [...by].map(([k, list]) => ({ k, name: originName(k), list: list.sort(byDl) }));
    return out.sort((x, y) => y.list.length - x.list.length || x.name.localeCompare(y.name));
  }
  // This week's chart when it has come; until then (or without one) the most downloaded.
  function ranked() {
    const all = pool(), chart = trendList(all).map((x) => x.a);
    return chart.length ? chart : [...all].sort(byDl).slice(0, 20);
  }
  // The line under a game in a list: its console, then how many play it.
  function listLine(a) {
    const n = Number(a.players) || 0;
    const who = n > 0 ? tn(n, '1 player', '{n} players') : a.downloads ? tf('{n} downloads', { n: fmtN(a.downloads) }) : '';
    return `${a.origin ? conDot(a.origin) + ' ' : ''}${esc([conName(a), who].filter(Boolean).join(' · '))}`;
  }
  // What a download is doing right now, in a few words.
  function jobText(id) {
    const j = jobOf(id);
    if (!j) return '';
    const [l, r] = progLabel(j);
    return [l, r].filter(Boolean).join(' · ');
  }
  const ring = (id, sz) => {
    const r = sz / 2 - 3, c = 2 * Math.PI * r, off = c * (1 - pct(jobOf(id)) / 100);
    return `<svg class="ph-ring" width="${sz}" height="${sz}" viewBox="0 0 ${sz} ${sz}"><circle class="tr" cx="${sz / 2}" cy="${sz / 2}" r="${r}"/><circle class="pr" data-pr="${esc(id)}" data-c="${c}" cx="${sz / 2}" cy="${sz / 2}" r="${r}" stroke-dasharray="${c}" stroke-dashoffset="${off}"/></svg>`;
  };

  const card = (a, sub) => `<button type="button" class="ph-gcard ph-press" data-open="${esc(a.id)}" data-key="g:${esc(a.id)}">${coverBox(a)}<span class="ph-gname">${esc(a.name)}</span><span class="ph-gsub">${sub == null ? listLine(a) : sub}</span></button>`;
  // The small button beside a game in a list, as an app store has it: Get installs right there,
  // Open starts the game, and a download shows its ring (a tap cancels it).
  function getBtn(a) {
    const id = esc(a.id);
    switch (stateOf(a)) {
      case 'busy': return `<button type="button" class="ph-gb" data-act="cancel" data-id="${id}" aria-label="${esc(t('Cancel'))}"><span class="ph-ringbox">${ring(a.id, 26)}</span></button>`;
      case 'paused': return `<button type="button" class="ph-gb op ph-press" data-act="resume" data-id="${id}">${esc(t('Resume'))}</button>`;
      case 'update': return `<button type="button" class="ph-gb op ph-press" data-act="update" data-id="${id}">${esc(t('Update'))}</button>`;
      case 'installed': case 'running': return `<button type="button" class="ph-gb op ph-press" data-act="play" data-id="${id}">${esc(t('Open'))}</button>`;
      case 'get': return `<button type="button" class="ph-gb ph-press" data-act="install" data-id="${id}">${esc(t('Get'))}</button>`;
      case 'uninstalling': case 'settingup': return '<span class="ph-gb"><span class="ph-spin s"></span></span>';
      default: return '';
    }
  }
  function lrow(a, opt = {}) {
    const rank = opt.rank ? `<span class="ph-rank"${RANK_COLORS[opt.rank - 1] ? ` style="color:${RANK_COLORS[opt.rank - 1]}"` : ''}>${opt.rank}</span>` : '';
    return `<div class="ph-lrow" data-key="r:${esc(a.id)}"><button type="button" class="ph-lmain ph-press" data-open="${esc(a.id)}">${rank}${coverBox(a)}<span class="ph-col ph-grow"><span class="ph-gname">${esc(a.name)}</span><span class="ph-gsub">${opt.sub == null ? listLine(a) : opt.sub}</span></span></button>${opt.right == null ? getBtn(a) : opt.right}</div>`;
  }
  const empty = (icon, html) => `<div class="ph-empty"><span class="ph-ei">${ic(icon)}</span>${html}</div>`;
  const secHead = (title, more) => `<div class="ph-sech"><h2>${esc(title)}</h2>${more || ''}</div>`;
  const sw = (key, on, off) => `<button type="button" class="ph-sw" role="switch" aria-checked="${on ? 'true' : 'false'}" data-sw="${esc(key)}"${off ? ' disabled' : ''}><i></i></button>`;
  const seg = (key, opts, i, big) => `<div class="ph-sg${big ? ' big' : ''}" data-sg="${key}" style="--n:${opts.length};--i:${i}"><span class="th"></span>${opts.map((o, n) => `<button type="button" class="${n === i ? 'on' : ''}" data-sgi="${n}">${o}</button>`).join('')}</div>`;

  // Android while it's for testers only: why the list is empty, and Sign In when that's the fix.
  function gate() {
    if (!S.listGate) return '';
    const signed = S.account && S.account.signedIn;
    return `<div class="ph-panel ph-box"><b>${esc(t('Android games'))}</b><p>${esc(t(S.listGate))}</p>${signed ? '' : `<button type="button" class="ph-btn pri sm" data-act="signin">${esc(t('Sign in'))}</button>`}</div>`;
  }

  // ---------- Home ----------
  // The banner's games: the ones picked on the website, else the best known with a wide picture.
  function featured() {
    const all = pool();
    const picked = window.bannerApps && window.bannerApps();
    if (picked) return picked.slice(0, 5);
    const wide = all.filter((a) => a.hero).sort(byDl);
    const lead = all.filter((a) => a.featured && (a.hero || a.cover));
    return uniq([...lead, ...wide, ...[...all].sort(byDl)].map((a) => a.id)).map(byId).slice(0, 5);
  }
  function avatarBtn() {
    const u = S.account && S.account.signedIn ? S.account.user : null;
    const inner = u && u.avatar ? img(u.avatar) : u ? esc((u.username || '?').slice(0, 1).toUpperCase()) : ic('user', 's');
    return `<button type="button" class="ph-ibtn ph-avatar ph-press" data-sub="account" aria-label="${esc(t('Account'))}">${inner}</button>`;
  }
  // The banner's main button says what a tap on the game's page would offer.
  function heroWord(a) {
    const st = stateOf(a);
    if (st === 'installed' || st === 'running') return ic('play', 's') + esc(t('Play'));
    if (st === 'update') return esc(t('Update'));
    return esc([t('Get'), sizeOf(a)].filter(Boolean).join(' · '));
  }
  function heroSlide(a) {
    const info = window.bannerInfo ? window.bannerInfo(a.id) : {};
    const kick = [info.tag || (a.featured ? t('Featured') : t('Popular right now')), conName(a)].filter(Boolean).join(' · ');
    const text = info.text || a.blurb || '';
    const title = a.logo ? img(media(a.logo), 'hl', 'data-logo') + `<span class="ph-disp ht" hidden>${esc(a.name)}</span>` : `<span class="ph-disp ht">${esc(a.name)}</span>`;
    return `<button type="button" data-open="${esc(a.id)}" data-soft="1" data-key="h:${esc(a.id)}">${img(heroUrl(a), 'bg')}<span class="tint"></span>
      <span class="info"><span class="ph-tag dark">${a.origin ? conDot(a.origin) : ''}${esc(kick)}</span>${title}${text ? `<span class="hb" dir="auto">${esc(text)}</span>` : ''}
      <span class="acts"><span class="ph-btn pri">${heroWord(a)}</span><span class="ph-btn sec">${esc(t('Details'))}</span></span></span></button>`;
  }
  function continueRow() {
    const mine = Object.values(S.library).sort((x, y) => (y.last_played || 0) - (x.last_played || 0) || (y.installed_at || 0) - (x.installed_at || 0)).map(libApp).filter(Boolean).slice(0, 14);
    if (!mine.length) return '';
    const one = (a) => `<button type="button" class="ph-wide ph-press" data-open="${esc(a.id)}" data-soft="1" data-key="w:${esc(a.id)}">${img(heroUrl(a), 'bg')}<span class="ph-shade"></span><span class="foot"><span class="ph-col ph-grow"><span class="ph-gname">${esc(a.name)}</span><span class="ph-gsub">${esc(playedFor(S.library[a.id]))}</span></span><span class="ph-playdot">${ic('play', 's')}</span></span></button>`;
    return `<section class="ph-sec" data-key="continue">${secHead(t('Continue playing'), `<button type="button" data-tab="library">${esc(t('Library'))}</button>`)}<div class="ph-hs">${mine.map(one).join('')}</div></section>`;
  }
  function row(key, title, list, sub, more) {
    if (!list.length) return '';
    return `<section class="ph-sec" data-key="${key}">${secHead(title, more)}<div class="ph-hs">${list.slice(0, 14).map((a) => card(a, sub ? sub(a) : null)).join('')}</div></section>`;
  }
  function viewHome() {
    const all = pool();
    if (!all.length) {
      const why = S.listGate ? gate() : empty('home', esc(S.offline ? t("Can't reach recomplauncher.com right now.") : t('Loading…')));
      return `<header class="ph-vhd" data-key="hd"><h1 class="ph-disp ph-vt">RecompLauncher</h1></header>${why}${continueRow()}`;
    }
    const feats = featured(), top = ranked();
    const seeAll = `<button type="button" data-tab="catalog">${esc(t('See all'))}</button>`;
    const chart = top.slice(0, 10).map((a, i) => `<button type="button" class="ph-rk ph-press" data-open="${esc(a.id)}" data-key="k:${esc(a.id)}" aria-label="${esc(a.name)}"><span class="num">${i + 1}</span>${coverBox(a)}</button>`).join('');
    return `<div class="ph-hhd" data-key="hd"><div>${mark(30)}<div class="ph-grow"><small>${esc(t('Welcome back'))}</small><b>${esc(t('Ready to play'))}</b></div>
      <button type="button" class="ph-ibtn glass ph-press" data-act="search" aria-label="${esc(t('Search'))}">${ic('search', 's')}</button>${avatarBtn()}</div></div>
      <div class="ph-hero" data-key="hero">${feats.map(heroSlide).join('')}</div>
      <div class="ph-dots" data-key="dots">${feats.map((a, i) => `<i class="${i === (P.slide || 0) ? 'on' : ''}"></i>`).join('')}</div>
      ${continueRow()}
      <section class="ph-sec" data-key="trend">${secHead(t('Trending this week'), `<button type="button" data-sub="trend">${esc(tf('Top {n}', { n: top.length }))}</button>`)}<div class="ph-hs" style="gap:6px">${chart}</div></section>
      ${row('popular', t('Popular on Android'), [...all].sort(byDl), null, seeAll)}
      ${row('updated', t('Recently updated'), recentlyUpdated(all), (a) => esc(a.version ? 'v' + String(verText(a.version)).replace(/^v/i, '') : ''), seeAll)}
      ${row('new', t('Just added'), [...all].sort(byNew), null, seeAll)}`;
  }

  // ---------- Catalog ----------
  function conTile(c, wide) {
    const pics = c.list.slice(0, wide ? 3 : 1).map((a) => img(coverUrl(a, 160))).join('');
    const count = wide ? `<small>${esc(tn(c.list.length, '1 game', '{n} games'))}</small>` : '';
    return `<button type="button" class="ph-con ph-press" data-con="${esc(c.k)}" data-key="c:${esc(c.k)}" style="--c:${conColor(c.k)}"><span class="glow"></span>${pics}<span class="k">${conDot(c.k, true)}<b>${esc(c.name)}</b>${count}</span></button>`;
  }
  function viewCatalog() {
    const all = pool(), cons = consoles(), top = ranked();
    const head = `<header class="ph-vhd" data-key="hd"><h1 class="ph-disp ph-vt">${esc(t('Catalog'))}</h1><p class="ph-vs">${esc(tf('{games} across {consoles}', { games: tn(all.length, '1 game', '{n} games'), consoles: tn(cons.length, '1 console', '{n} consoles') }))}</p></header>`;
    if (!all.length) return head + (gate() || empty('catalog', esc(S.offline ? t("Can't reach recomplauncher.com right now.") : t('No games yet'))));
    const az = [...all].sort((x, y) => x.name.localeCompare(y.name));
    return `${head}
      <div class="ph-padx" data-key="find" style="margin-top:6px"><button type="button" class="ph-sfield ph-press" data-act="search">${ic('search')}<span>${esc(t('Search games, consoles, developers'))}</span></button></div>
      <section class="ph-sec" data-key="cons">${secHead(t('Consoles'), `<button type="button" data-sub="cons">${esc(t('See all'))}</button>`)}<div class="ph-cons">${cons.slice(0, 4).map((c) => conTile(c)).join('')}</div></section>
      <section class="ph-sec" data-key="trend">${secHead(t('Trending this week'), `<button type="button" data-sub="trend">${esc(tf('Top {n}', { n: top.length }))}</button>`)}${top.slice(0, 5).map((a, i) => lrow(a, { rank: i + 1 })).join('')}</section>
      <section class="ph-sec" data-key="all">${secHead(t('All games'))}<div class="ph-grid long">${az.map((a) => card(a)).join('')}</div></section>`;
  }

  // ---------- Library ----------
  // The player's games: installed ones, the latest played first, then ones still on their way.
  function mineList() {
    const have = Object.values(S.library).sort((x, y) => (y.last_played || 0) - (x.last_played || 0) || (y.installed_at || 0) - (x.installed_at || 0)).map(libApp).filter(Boolean);
    const coming = Object.keys(S.jobs).filter((id) => !S.library[id]).map(appOf).filter(Boolean);
    return [...coming, ...have];
  }
  function libLine(a) {
    const st = stateOf(a);
    if (st === 'update') return `<i class="ph-dot" style="--c:#34F2E8"></i> ${esc(t('Update ready'))}`;
    if (st === 'busy') return esc(S.library[a.id] ? t('Updating') : t('Downloading'));
    if (st === 'paused') return esc(t('Paused'));
    return esc(playedFor(S.library[a.id]));
  }
  function libOver(a) {
    if (isBusy(a)) return `<span class="ph-ringp">${ring(a.id, 46)}</span>`;
    if (stateOf(a) === 'update') return `<span class="ph-badge"><i class="ph-dot" style="--c:#34F2E8"></i>${esc(t('Update'))}</span>`;
    return '';
  }
  function dlRow(a) {
    const j = jobOf(a.id), paused = stateOf(a) === 'paused';
    const right = paused ? `<button type="button" class="ph-gb op ph-press" data-act="resume" data-id="${esc(a.id)}">${esc(t('Resume'))}</button>` : `<button type="button" class="ph-ibtn solid ph-press" data-act="cancel" data-id="${esc(a.id)}" aria-label="${esc(t('Cancel'))}">${ic('x', 's')}</button>`;
    return `<div class="ph-lrow ph-dlrow" data-key="d:${esc(a.id)}"><button type="button" class="ph-lmain ph-press" data-open="${esc(a.id)}"><span class="pic">${coverBox(a, 'dim')}<span class="ph-ringp">${ring(a.id, 36)}</span></span><span class="ph-col ph-grow"><span class="ph-gname">${esc(a.name)}</span><span class="ph-bar"><i data-pw="${esc(a.id)}" style="width:${pct(j)}%"></i></span><span class="ph-gsub" data-pt="${esc(a.id)}">${esc(jobText(a.id))}</span></span></button>${right}</div>`;
  }
  function paneDownloads() {
    const ids = uniq([...Object.keys(S.jobs), ...Object.keys(S.paused || {})]);
    const list = ids.map(appOf).filter(Boolean);
    if (!list.length) return empty('downloads', `${esc(t('Nothing is downloading.'))}<br>${esc(t('Downloads keep going while you browse.'))}`);
    return list.map(dlRow).join('');
  }
  function updatesList() {
    return Object.keys(S.library).map(appOf).filter((a) => a && stateOf(a) === 'update');
  }
  function paneUpdates() {
    const ups = updatesList();
    if (!ups.length) return empty('check', esc(t('Everything is up to date.')));
    const one = (a) => lrow(a, { sub: esc([a.version ? tf('Version {v}', { v: verText(a.version) }) : '', sizeOf(a)].filter(Boolean).join(' · ')), right: `<button type="button" class="ph-btn pri sm ph-press" data-act="update" data-id="${esc(a.id)}">${esc(t('Update'))}</button>` });
    return `<div class="ph-libh"><span class="ph-mut">${esc(tn(ups.length, '1 update ready', '{n} updates ready'))}</span>${ups.length > 1 ? `<button type="button" class="ph-btn sec sm ph-press" data-act="update-all">${esc(t('Update all'))}</button>` : ''}</div>${ups.map(one).join('')}`;
  }
  function paneGames() {
    const mine = mineList();
    const savedList = savedItems().map((x) => x.a).filter((a) => !a.soon && forOS(a));
    if (!mine.length && !savedList.length) return gate() + empty('library', esc(t('Your games show up here once you install one.')));
    const last = mine.find((a) => S.library[a.id] && S.library[a.id].last_played && a.hero && !isBusy(a));
    const jump = last ? `<button type="button" class="ph-jump ph-press" data-open="${esc(last.id)}" data-soft="1" data-key="jump">${img(heroUrl(last), 'bg')}<span class="ph-shade"></span><span class="foot"><span class="ph-col ph-grow"><span class="eye">${esc(t('Jump back in'))}</span><span class="ph-disp">${esc(last.name)}</span><small>${esc(playedFor(S.library[last.id]))}</small></span><span class="ph-btn pri sm">${ic('play', 's')}${esc(t('Play'))}</span></span></button>` : '';
    const grid = pref.libview() === 'grid';
    const tile = (a) => `<button type="button" class="ph-gcard ph-press" data-open="${esc(a.id)}" data-key="g:${esc(a.id)}"><span style="position:relative;display:block">${coverBox(a, isBusy(a) ? 'dim' : '')}${libOver(a)}</span><span class="ph-gname">${esc(a.name)}</span><span class="ph-gsub">${libLine(a)}</span></button>`;
    const line = (a) => lrow(a, { sub: libLine(a) });
    const games = mine.length ? `<div class="ph-libh" data-key="lh"><b>${esc(t('All games'))} <span>${mine.length}</span></b><button type="button" class="ph-ibtn solid ph-press" data-act="libview" aria-label="${esc(t('Change view'))}">${ic(grid ? 'list' : 'library', 's')}</button></div>${grid ? `<div class="ph-grid" data-key="lg">${mine.map(tile).join('')}</div>` : mine.map(line).join('')}` : '';
    const later = savedList.length ? `<div class="ph-libh" data-key="sh"><b>${esc(t('Saved for later'))} <span>${savedList.length}</span></b></div>${savedList.map((a) => lrow(a)).join('')}` : '';
    return jump + games + later;
  }
  function viewLibrary() {
    const n = Object.keys(S.library).length;
    const bytes = Object.keys(S.library).map(appOf).reduce((sum, a) => sum + (a ? parseSize(sizeOf(a)) : 0), 0);
    const sub = [tn(n, '1 game', '{n} games'), bytes ? tf('{size} on this device', { size: fmtB(bytes) }) : ''].filter(Boolean).join(' · ');
    const dls = uniq([...Object.keys(S.jobs), ...Object.keys(S.paused || {})]).length, ups = updatesList().length;
    const count = (c) => (c ? `<span class="ph-cnt">${c}</span>` : '');
    return `<header class="ph-vhd" data-key="hd"><h1 class="ph-disp ph-vt">${esc(t('Library'))}</h1><p class="ph-vs">${esc(sub)}</p></header>
      <div class="ph-padx" data-key="seg" style="margin-top:6px">${seg('lib', [esc(t('Games')), esc(t('Downloads')) + count(dls), esc(t('Updates')) + count(ups)], P.libi, true)}</div>
      <div class="ph-lpw" data-key="panes"><div class="ph-lp" id="ph-lp" style="transform:translateX(${-P.libi * 100}%)"><div>${paneGames()}</div><div>${paneDownloads()}</div><div>${paneUpdates()}</div></div></div>`;
  }

  // ---------- Coming Soon and Downloads, as tabs of their own ----------
  function viewSoon() {
    const list = visible().filter((a) => a.soon && forOS(a)).sort((x, y) => Number(isSaved(y.id)) - Number(isSaved(x.id)) || x.name.localeCompare(y.name));
    const head = `<header class="ph-vhd" data-key="hd"><h1 class="ph-disp ph-vt">${esc(t('Coming Soon'))}</h1><p class="ph-vs">${esc(t('Save a game and we\'ll let you know when it comes out.'))}</p></header>`;
    if (!list.length) return head + empty('soon', esc(t('Nothing is announced right now.')));
    const save = (a) => `<button type="button" class="ph-gb${isSaved(a.id) ? ' op' : ''} ph-press" data-act="save" data-id="${esc(a.id)}">${esc(isSaved(a.id) ? t('Saved') : t('Save'))}</button>`;
    return head + list.map((a) => lrow(a, { right: save(a) })).join('');
  }
  // ---------- Saves: the copies kept after each game, and cloud saves ----------
  // Android keeps every app's own folders to itself: only games that keep their saves in the phone's
  // shared storage (the catalog's android_saves) can be copied, and only once the player allows All
  // files access. A phone's cloud copies are its own, apart from a PC's (cloud.rs, cloud_id).
  const savesLoad = () => { if (window.loadSavesAll) window.loadSavesAll().then(soon); };
  async function filesCheck() {
    if (S.os !== 'android') { P.files = true; return; }
    let now = P.files;
    try { now = !!(await invoke('android_files_access')).granted; } catch (e) { /* asked again next time */ }
    if (now !== P.files) { P.files = now; S.savesAll = null; soon(); }
  }
  function svBadge(x) {
    if (x.state === 'conflict') return `<span class="ph-svb warn">${esc(t('Sync Now'))}</span>`;
    if (x.cloudOn === true && x.pending && (S.account || {}).signedIn) return `<span class="ph-svb">${esc(t('Going up'))}</span>`;
    if (x.cloudOn === true && (S.account || {}).signedIn) return `<span class="ph-svb ok">${ic('cloud', 'xs')}${esc(t('In the cloud'))}</span>`;
    return `<span class="ph-svb">${esc(t('Only on this phone'))}</span>`;
  }
  function viewSaves() {
    const head = `<header class="ph-vhd" data-key="hd"><h1 class="ph-disp ph-vt">${esc(t('Saves'))}</h1><p class="ph-vs">${esc(t('After every session the launcher keeps a copy. Restore an older one any time.'))}</p></header>`;
    const o = S.savesAll;
    if (!o) { savesLoad(); return head + `<div class="ph-row ph-padx" style="gap:12px;padding-top:26px"><span class="ph-spin"></span><span class="ph-mut">${esc(t('Loading…'))}</span></div>`; }
    const acc = S.account || {};
    const games = (o.games || []).map((x) => ({ ...x, a: appOf(x.id) })).filter((x) => x.a);
    const known = games.filter((x) => x.known !== false);
    const out = games.length - known.length;
    const withSaves = known.filter((x) => x.copies).sort((x, y) => ((y.newest || {}).when || 0) - ((x.newest || {}).when || 0));
    const rest = known.filter((x) => !x.copies);
    const copies = withSaves.reduce((n, x) => n + x.copies, 0), size = withSaves.reduce((n, x) => n + x.size, 0);
    const inCloud = known.filter((x) => x.cloudOn === true).length;
    const used = acc.used || 0, quota = acc.quota || 150 * 1024 * 1024;
    const meter = (frac, cls) => { const on = frac > 0 ? Math.max(1, Math.round(Math.min(1, frac) * 16)) : 0; return `<div class="ph-meter ${cls}">${Array.from({ length: 16 }, (_, i) => `<i${i < on ? ' class="on"' : ''} style="animation-delay:${i * 25}ms"></i>`).join('')}</div>`; };
    const access = known.length && P.files === false
      ? `<div class="ph-panel ph-svacc" data-key="acc"><span class="ph-svi">${ic('key', 's')}</span><div class="ph-col ph-grow"><b>${esc(t('Allow All files access'))}</b><span class="ph-mut">${esc(t('Android asks first. Allow it once and the launcher can copy your games\' saves.'))}</span><button type="button" class="ph-btn pri sm ph-press" data-act="files">${esc(t('Open Settings'))}</button></div></div>` : '';
    const phoneCard = `<div class="ph-panel ph-svc"><div class="ph-row" style="gap:10px"><span class="ph-svi">${ic('phone', 's')}</span><div class="ph-col"><b>${esc(t('On this phone'))}</b><small>${esc(tn(withSaves.length, '1 game with saves', '{n} games with saves'))}</small></div></div>${meter(known.length ? withSaves.length / known.length : 0, 'a')}<div class="ph-row ph-svn"><b>${esc(fmtB(size) || '0 KB')}</b><small>${esc(tn(copies, '1 copy kept', '{n} copies kept'))}</small></div></div>`;
    const cloudCard = acc.signedIn
      ? `<div class="ph-panel ph-svc"><div class="ph-row" style="gap:10px"><span class="ph-svi">${ic('cloud', 's')}</span><div class="ph-col"><b>${esc(t('Cloud saves'))}</b><small>${esc(tn(inCloud, '1 game backs up to your account', '{n} games back up to your account'))}</small></div></div>${meter(used / quota, 'b')}<div class="ph-row ph-svn"><b class="ph-svq">${esc(tf('{used} of {quota} used', { used: fmtB(used) || '0 KB', quota: fmtB(quota) }))}</b></div></div>`
      : `<div class="ph-panel ph-svc"><div class="ph-row" style="gap:10px"><span class="ph-svi">${ic('cloud', 's')}</span><div class="ph-col"><b>${esc(t('Cloud saves'))}</b><small>${esc(t('Sign in to keep your saves in the cloud.'))}</small></div></div><button type="button" class="ph-btn pri sm wide ph-press" data-act="signin" style="margin-top:14px">${esc(t('Sign in'))}</button></div>`;
    const auto = acc.signedIn ? `<div class="ph-panel ph-set" data-key="auto"><div class="ph-srow"><div class="t"><b>${esc(t('Back up to the cloud'))}</b><span>${esc(t('After you play, a copy goes to your account'))}</span></div>${sw('cloudall', ui().cloud_all === true)}</div></div>` : '';
    const row = (x) => { const n = x.newest || {}; return `<div class="ph-panel ph-svrow" data-key="sv:${esc(x.id)}"><button type="button" class="ph-press" data-act="copies" data-id="${esc(x.id)}">${coverBox(x.a)}<span class="ph-col ph-grow" style="gap:4px;min-width:0"><span class="ph-gname">${esc(x.a.name)}</span><span class="ph-gsub">${esc(tf('Last saved {when}', { when: ago(n.when) }))}</span>${svBadge(x)}</span><span class="ph-col ph-svm"><b>${esc(tn(x.copies, '1 copy', '{n} copies'))}</b><small>${esc(fmtB(x.size))}</small></span></button><button type="button" class="ph-ibtn solid ph-press" data-act="backup" data-id="${esc(x.id)}" aria-label="${esc(t('Back up now'))}"${S.running && S.running.has && S.running.has(x.id) ? ' disabled' : ''}>${ic('up', 's')}</button></div>`; };
    const quiet = (x) => `<div class="ph-lrow" data-key="sq:${esc(x.id)}"><button type="button" class="ph-lmain ph-press" data-open="${esc(x.id)}">${coverBox(x.a)}<span class="ph-col ph-grow"><span class="ph-gname">${esc(x.a.name)}</span><span class="ph-gsub">${esc(x.found ? t('Saves found, copied after you play') : t('Not played yet'))}</span></span></button></div>`;
    const list = withSaves.length ? `<section class="ph-sec ph-svsec" data-key="svh">${secHead(t('Games'))}${withSaves.map(row).join('')}</section>` : '';
    const later = rest.length ? `<section class="ph-sec ph-svsec" data-key="svq">${secHead(t('No saves yet'))}${rest.map(quiet).join('')}</section>` : '';
    const none = !known.length ? empty('saves', esc(t('Games that keep their saves in the phone\'s shared storage show up here once you install one.'))) : '';
    const away = out ? `<p class="ph-mut ph-padx ph-svout" data-key="out">${esc(tn(out, '1 game keeps its saves where Android doesn\'t let the launcher in.', '{n} games keep their saves where Android doesn\'t let the launcher in.'))}</p>` : '';
    return head + access + `<div class="ph-svcards ph-padx" data-key="cards">${phoneCard}${cloudCard}</div>` + auto + list + later + none + away;
  }
  // A game's copies, on this phone and in the cloud; one tapped asks before it's put back.
  async function copiesSheet(id, pick) {
    const a = appOf(id);
    if (!a) return;
    P.copies = id;
    if (!pick) sheet(sheetHead(a) + `<div class="ph-row" style="gap:12px;padding:20px 0"><span class="ph-spin"></span><span class="ph-mut">${esc(t('Loading…'))}</span></div>`);
    let info;
    try { info = await invoke('saves_info', { id }); } catch (e) { closeSheet(); return fail(e); }
    if (P.copies !== id || !P.sheet) return;
    const line = (c, cloud) => `<button type="button" class="ph-step ph-press" data-act="restore" data-id="${esc(id)}" ${cloud ? `data-v="${esc(c.v)}"` : `data-name="${esc(c.name)}"`}>${ic(cloud ? 'cloud' : 'saves', 's')}<span class="ph-col ph-grow" style="gap:2px"><b>${esc(agoCap(cloud ? c.made : c.when))}</b><small class="ph-mut">${esc([c.device, fmtB(c.size)].filter(Boolean).join(' · '))}</small></span><span class="ph-mut" style="font-size:12.5px">${esc(t('Restore'))}</span></button>`;
    const local = info.local || [], cloud = (info.cloud || []).filter(() => (S.account || {}).signedIn);
    const body = (local.length ? `<p class="ph-eyebrow">${esc(t('On this phone'))}</p>${local.map((c) => line(c, false)).join('')}` : '')
      + (cloud.length ? `<p class="ph-eyebrow" style="margin-top:14px">${esc(t('Cloud saves'))}</p>${cloud.map((c) => line(c, true)).join('')}` : '')
      + (!local.length && !cloud.length ? `<p class="ph-mut">${esc(t('Not played yet'))}</p>` : '');
    paint(el.sheet, '<div class="ph-grab"></div>' + sheetHead(a) + body);
  }
  function restoreAsk(b) {
    const a = appOf(b.dataset.id);
    if (!a) return;
    const attr = b.dataset.v ? `data-v="${esc(b.dataset.v)}"` : `data-name="${esc(b.dataset.name)}"`;
    paint(el.sheet, `<div class="ph-grab"></div>${sheetHead(a)}<h2 class="ph-disp ph-svh">${esc(t('Restore this save?'))}</h2><p class="ph-mut ph-svp">${esc(t('The game goes back to this save. What\'s there now is kept as a copy first, so you can switch back.'))}</p>
      <button type="button" class="ph-btn pri wide ph-press" data-act="restore-go" data-id="${esc(a.id)}" ${attr} style="margin-top:16px">${esc(t('Restore'))}</button><button type="button" class="ph-btn sec wide ph-press" data-close="1" style="margin-top:10px">${esc(t('Cancel'))}</button>`);
  }

  const viewDownloads = () => `<header class="ph-vhd" data-key="hd"><h1 class="ph-disp ph-vt">${esc(t('Downloads'))}</h1><p class="ph-vs">${esc(t('Downloads keep going while you browse.'))}</p></header>${paneDownloads()}`;

  // ---------- Settings ----------
  function viewSettings() {
    const acc = S.account || {}, user = acc.signedIn && acc.user ? acc.user : null;
    const face = user && user.avatar ? img(user.avatar) : user ? esc((user.username || '?').slice(0, 1).toUpperCase()) : ic('user');
    const mine = Object.keys(S.library).map(appOf).filter(Boolean);
    const bytes = mine.map((a) => parseSize(sizeOf(a)));
    const total = bytes.reduce((x, y) => x + y, 0);
    const lang = (LANGS.find((l) => l[0] === ui().lang) || [])[1] || t('Same as your phone');
    const srow = (title, sub, right) => `<div class="ph-srow"><div class="t"><b>${esc(title)}</b>${sub ? `<span>${esc(sub)}</span>` : ''}</div>${right || ''}</div>`;
    const link = (icon, title, sub, attr) => `<button type="button" class="ph-srow ph-press" ${attr}>${ic(icon, 's')}<div class="t"><b>${esc(title)}</b>${sub ? `<span>${esc(sub)}</span>` : ''}</div><span class="ph-mut">${ic('chev', 's')}</span></button>`;
    const tabs = pref.tabs();
    const tabRow = (k) => `<div class="ph-srow">${ic(k, 's')}<div class="t"><b>${esc(t(TABS[k]))}</b></div>${sw('tab:' + k, tabs.includes(k), TABS_FIXED.includes(k))}</div>`;
    const stor = (a, i) => `<div class="ph-srow" data-key="st:${esc(a.id)}"><span class="thumb">${img(coverUrl(a, 160))}</span><div class="t"><b>${esc(a.name)}</b><span>${esc(sizeOf(a))}</span></div><button type="button" class="ph-ibtn ph-press ph-mut" data-act="uninstall" data-id="${esc(a.id)}" aria-label="${esc(tf('Uninstall {name}', { name: a.name }))}">${ic('trash', 's')}</button></div>`;
    return `<header class="ph-vhd" data-key="hd"><h1 class="ph-disp ph-vt">${esc(t('Settings'))}</h1></header>
      <button type="button" class="ph-panel ph-set link ph-press" data-sub="account" data-key="acc"><span class="ph-ibtn ph-avatar big">${face}</span><span class="ph-col ph-grow"><b>${esc(user ? user.username : t('Sign in'))}</b><small>${esc(user ? t('Signed in. Your saved games follow you.') : t('Keep your saved games with your account.'))}</small></span><span class="ph-mut">${ic('chev', 's')}</span></button>
      <div class="ph-panel ph-set" data-key="look"><h3>${esc(t('Appearance'))}</h3>
        ${srow(t('Theme'), t('OLED is pure black to save battery'), `<div class="ph-pick" style="width:188px">${seg('theme', [esc(tc('Dark|theme')), 'OLED', esc(tc('Light|theme'))], THEMES.indexOf(pref.theme()))}</div>`)}
        ${srow(t('Display size'), '', `<div class="ph-pick" style="width:132px">${seg('size', ['S', 'M', 'L'], ['s', 'm', 'l'].indexOf(pref.size()))}</div>`)}
        ${srow(t('Menu labels'), t('Show names under the menu icons'), sw('labels', pref.labels()))}
        ${srow(t('Vibrate on taps'), '', sw('haptics', pref.haptics()))}</div>
      <div class="ph-panel ph-set" data-key="menu"><h3>${esc(t('Menu bar'))}</h3>
        <div class="ph-srow" style="min-height:0;padding:8px 0 4px"><span class="ph-mut" style="font-size:12.5px">${esc(tf('Pick 3 to 5 tabs. {n} chosen.', { n: tabs.length }))}</span></div>
        ${Object.keys(TABS).map(tabRow).join('')}</div>
      <div class="ph-panel ph-set" data-key="bp"><h3>${esc(t('Big Picture mode'))}</h3>
        ${srow(t('Open with a controller'), t('Switches when you press a button on one'), sw('padtv', pref.padTv()))}
        <div class="ph-srow"><button type="button" class="ph-btn sec wide ph-press" data-act="tv">${ic('tv', 's')}${esc(t('Open Big Picture now'))}</button></div></div>
      <div class="ph-panel ph-set" data-key="cat"><h3>${esc(t('Catalog'))}</h3>
        ${srow(t('Hide AI-assisted recomps'), t('Recomps whose developers used AI tools for code, art, writing or voice won\'t show in the Catalog.'), sw('hideai', !!S.settings.hide_ai))}</div>
      ${mine.length ? `<div class="ph-panel ph-set" data-key="stor"><h3>${esc(t('Storage'))}</h3><div class="ph-storh"><b>${esc(total ? fmtB(total) : '')}</b><span>${esc(tn(mine.length, '1 game', '{n} games'))}</span></div>
        ${total ? `<div class="ph-stor">${mine.map((a, i) => `<i style="flex:${(bytes[i] || 0) / total};--c:${conColor(a.origin)};animation-delay:${i * 60}ms"></i>`).join('')}</div>` : ''}${mine.map(stor).join('')}</div>` : ''}
      <div class="ph-panel ph-set" data-key="about"><h3>${esc(t('About'))}</h3>
        ${link('globe', t('Language'), lang, 'data-act="lang"')}
        ${link('chat', 'Discord', t('Help, news and other players'), 'data-act="discord"')}
        ${link('home', 'recomplauncher.com', '', 'data-act="site"')}
        ${srow(tf('RecompLauncher {v}', { v: S.version || '' }), engine(), '')}</div>`;
  }

  const VIEW = { home: viewHome, catalog: viewCatalog, library: viewLibrary, downloads: viewDownloads, saves: viewSaves, soon: viewSoon, settings: viewSettings };

  // ---------- a game's page ----------
  // The main button for a game: Install, the download's progress, Update, Play.
  function ctaHTML(a) {
    const id = esc(a.id), st = stateOf(a), j = jobOf(a.id);
    const more = `<button type="button" class="ph-ibtn solid ph-press" data-menu="${id}" aria-label="${esc(t('More options'))}">${ic('more')}</button>`;
    const saveBtn = `<button type="button" class="ph-ibtn solid ph-press${isSaved(a.id) ? ' on' : ''}" data-act="save" data-id="${id}" aria-label="${esc(isSaved(a.id) ? t('Saved') : t('Save for later'))}">${ic('bookmark')}</button>`;
    if (st === 'busy' && j && j.phase === 'download' && j.total) return `<div class="ph-dl"><i data-pw="${id}" style="width:${pct(j)}%"></i><span>${esc(t('Downloading'))}</span><span data-pt="${id}">${esc(jobText(a.id))}</span></div><button type="button" class="ph-rbtn ph-press" data-act="cancel" data-id="${id}" aria-label="${esc(t('Cancel'))}">${ring(a.id, 50)}${ic('x', 's')}</button>`;
    if (st === 'busy') return `<div class="ph-dl mid"><span class="ph-spin"></span><span data-pt="${id}">${esc(jobText(a.id) || t('Getting ready'))}</span></div><button type="button" class="ph-ibtn solid ph-press" data-act="cancel" data-id="${id}" aria-label="${esc(t('Cancel'))}">${ic('x', 's')}</button>`;
    if (st === 'paused') return `<button type="button" class="ph-btn pri ph-press" data-act="resume" data-id="${id}">${ic('downloads', 's')}${esc(t('Resume'))}</button><button type="button" class="ph-ibtn solid ph-press" data-act="cancel" data-id="${id}" aria-label="${esc(t('Cancel'))}">${ic('x', 's')}</button>`;
    if (st === 'installed' || st === 'running') return `<button type="button" class="ph-btn pri ph-press${P.just === a.id ? ' ph-burst' : ''}" data-act="play" data-id="${id}">${ic('play', 's')}${esc(t('Play'))}</button>${more}`;
    if (st === 'update') return `<button type="button" class="ph-btn pri ph-press" data-act="update" data-id="${id}">${ic('downloads', 's')}${esc(t('Update'))}</button><button type="button" class="ph-btn sec ph-press" data-act="play" data-id="${id}">${ic('play', 's')}${esc(t('Play'))}</button>`;
    if (st === 'uninstalling') return `<div class="ph-dl mid"><span class="ph-spin"></span><span>${esc(t('Uninstalling…'))}</span></div>`;
    if (st === 'soon') return `<button type="button" class="ph-btn pri" disabled>${esc(t('Coming soon'))}</button>${saveBtn}`;
    if (st === 'get') return `<button type="button" class="ph-btn pri ph-press" data-act="install" data-id="${id}">${ic('downloads', 's')}${esc([t('Install'), sizeOf(a)].filter(Boolean).join(' · '))}</button>${saveBtn}`;
    return `<button type="button" class="ph-btn pri" disabled>${esc(tf('No {os} version yet', { os: OS_NAME[S.os] || 'Android' }))}</button>`;
  }
  function gameHTML(a) {
    const id = esc(a.id), wide = !!a.hero;
    const stats = [
      [Number(a.players) > 0 ? fmtN(Number(a.players)) : '', t('players')],
      [sizeOf(a), t('size')],
      [a.downloads ? fmtN(a.downloads) : '', t('downloads')],
      [a.version ? String(verText(a.version)) : '', t('version')],
    ].filter(([v]) => v);
    const tags = tagsOf(a);
    const about = [a.blurb || notesLine(a.description), a.about && a.about.summary].filter(Boolean);
    const facts = [[t('Developer'), devName(a)], [t('Console'), conName(a)], [t('Genre'), GENRES[a.genre] ? t(GENRES[a.genre]) : ''], [t('Latest release'), a.released ? date(a.released) : '']].filter(([, v]) => v);
    const more = moreLike(a, 8);
    const based = a.game ? htmlFill(t('Based on {game}'), { game: esc(a.game) }) : '';
    const by = devName(a) ? htmlFill(t('by {who}'), { who: `<b>${esc(devName(a))}</b>` }) : '';
    return `<div class="ph-ptop"><button type="button" class="ph-ibtn glass ph-press" data-act="back" aria-label="${esc(t('Back'))}">${ic('back')}</button><button type="button" class="ph-ibtn glass ph-press" data-menu="${id}" aria-label="${esc(t('More options'))}">${ic('more', 's')}</button></div>
      <div class="ph-gph">${img(heroUrl(a), wide ? '' : 'blur', 'id="ph-gphimg"')}<div class="tint"></div></div>
      <div class="ph-gpb">
        <div class="ph-gpt full">${coverBox(a, '', 'ph-gpcover')}<div class="ph-col ph-grow"><div class="tags">${a.origin ? `<span class="ph-tag">${conDot(a.origin)}${esc(conName(a))}</span>` : ''}${GENRES[a.genre] ? `<span class="ph-tag">${esc(t(GENRES[a.genre]))}</span>` : ''}</div><h1 class="ph-disp" dir="auto">${esc(a.name)}</h1><p>${[based, by].filter(Boolean).join(' · ')}</p></div></div>
        ${stats.length ? `<div class="ph-stats">${stats.map(([v, k]) => `<div><b class="${v.length > 7 ? 'long' : ''}">${esc(v)}</b><span>${esc(k)}</span></div>`).join('')}</div>` : ''}
        <div class="ph-cta" data-cta="${id}">${ctaHTML(a)}</div>
        ${a.soon ? `<p class="ph-mut" style="font-size:13px;line-height:1.45">${esc(soonNoteText(a))}</p>` : ''}
        ${tags.length ? `<div class="ph-chips">${tags.map((x) => `<span class="ph-chip">${esc(x)}</span>`).join('')}</div>` : ''}
        ${about.length ? `<div class="ph-about"><h2>${esc(t('About'))}</h2>${about.map((p) => `<p dir="auto">${esc(p)}</p>`).join('')}</div>` : ''}
        ${a.repo && !a.local ? `<button type="button" class="ph-panel ph-note ph-press" data-readme="${id}"><span class="ib">${ic('file', 's')}</span><span class="ph-col ph-grow"><b>${esc(t('Read the README'))}</b><small>${esc(t('Notes, controls and known issues from the developer'))}</small></span><span class="ph-mut">${ic('chev', 's')}</span></button>` : ''}
        ${facts.length ? `<div class="ph-panel ph-facts aside">${facts.map(([k, v]) => `<div><span>${esc(k)}</span><b>${esc(v)}</b></div>`).join('')}</div>` : ''}
        ${more.length ? `<div class="ph-more full">${secHead(t('More like this'))}<div class="ph-hs">${more.map((x) => card(x)).join('')}</div></div>` : ''}
      </div>`;
  }

  // Where a rectangle sits in the design's own pixels (the frame is scaled).
  function rin(node) {
    const r = node.getBoundingClientRect(), u = el.ui.getBoundingClientRect();
    return { x: (r.left - u.left) / P.z, y: (r.top - u.top) / P.z, w: r.width / P.z, h: r.height / P.z };
  }
  function depth() {
    el.ui.classList.toggle('ph-deep', P.stack.length > 0);
    el.ui.classList.toggle('ph-side', P.subs.length > 0 && !P.stack.length);
  }
  // A cover flies from where it was tapped to its place on the game's page (and back).
  function fly(src, a, b, rad, done) {
    const f = document.createElement('img');
    f.src = src;
    f.className = 'ph-fly';
    f.style.width = a.w + 'px';
    f.style.height = a.h + 'px';
    f.style.borderRadius = rad + 'px';
    el.ui.appendChild(f);
    const frames = [{ transform: `translate(${a.x}px,${a.y}px) scale(1)` }, { transform: `translate(${b.x}px,${b.y}px) scale(${b.w / a.w},${b.h / a.h})` }];
    const an = f.animate(frames, { duration: 560, easing: 'cubic-bezier(.3,1.2,.4,1)', fill: 'forwards' });
    an.onfinish = () => { if (done) done(); f.remove(); };
  }
  // A banner grows into the top of the game's page, then fades into the page's own picture.
  function flyHero(srcImg, a, b, rad) {
    const from = rin(srcImg), f = document.createElement('img');
    f.src = heroUrl(a);
    f.className = 'ph-fly';
    f.style.cssText = `left:${from.x}px;top:${from.y}px;width:${from.w}px;height:${from.h}px;border-radius:${rad}px`;
    el.ui.appendChild(f);
    const start = { left: from.x + 'px', top: from.y + 'px', width: from.w + 'px', height: from.h + 'px', borderRadius: rad + 'px', opacity: 1 };
    const end = { left: b.x + 'px', top: b.y + 'px', width: b.w + 'px', height: b.h + 'px', borderRadius: '0px' };
    const frames = [start, Object.assign({ opacity: 1, offset: 0.72 }, end), Object.assign({ opacity: 0 }, end)];
    f.animate(frames, { duration: 700, easing: 'cubic-bezier(.3,1.08,.4,1)', fill: 'forwards' }).onfinish = () => f.remove();
  }
  function rise(gp) {
    gp.style.transform = 'translateY(46px) scale(.97)';
    void gp.offsetWidth;
    gp.style.transition = 'opacity 300ms ease,transform 600ms var(--soft)';
    gp.style.opacity = '1';
    gp.style.transform = 'none';
  }
  function openGame(id, from) {
    const a = appOf(id);
    if (!a) return toast(t('This recomp isn\'t listed anymore.'), true);
    buzz();
    if (P.stack.length) { P.stack.push(id); return swapGame(a, 1); }
    const gp = el.gp;
    P.stack = [id];
    gp.innerHTML = gameHTML(a);
    gp.classList.add('on');
    gp.style.cssText = 'transition:none;opacity:0;transform:none';
    // A game's page always opens at its top (set once it's on screen: a hidden page can't scroll).
    gp.scrollTop = 0;
    depth();
    loadReadme(a);
    const target = $('#ph-gpcover', gp);
    const soft = from && from.dataset.soft;
    const srcCover = from && !soft ? $('.ph-cover img.ld', from) : null;
    const srcWide = soft && a.hero && from.offsetWidth > el.ui.offsetWidth * 0.6 ? $('img.bg', from) : null;
    P.src = null;
    if (srcWide) {
      const b = rin($('.ph-gph', gp));
      gp.style.transform = 'translateY(34px)';
      void gp.offsetWidth;
      gp.style.transition = 'opacity 360ms ease 140ms,transform 640ms var(--soft)';
      gp.style.opacity = '1';
      gp.style.transform = 'none';
      return flyHero(srcWide, a, b, 20);
    }
    if (srcCover) {
      const srcBox = srcCover.parentNode;
      const from0 = rin(srcBox), to = rin(target);
      P.src = { layer: from.closest('.ph-view,.ph-sp,.ph-so'), id };
      target.style.visibility = 'hidden';
      srcBox.style.visibility = 'hidden';
      rise(gp);
      return fly(srcCover.currentSrc || srcCover.src, from0, to, 14, () => { target.style.visibility = ''; srcBox.style.visibility = ''; });
    }
    rise(gp);
    target.animate([{ transform: 'scale(.6)', opacity: 0 }, { transform: 'none', opacity: 1 }], { duration: 640, delay: 70, easing: 'cubic-bezier(.34,1.56,.64,1)', fill: 'backwards' });
  }
  // From one game's page to another (More like this), and back again.
  function swapGame(a, dir) {
    const gp = el.gp;
    const out = gp.animate([{ opacity: 1, transform: 'none' }, { opacity: 0, transform: `translateX(${-60 * dir}px) scale(.97)` }], { duration: 200, easing: 'ease-in' });
    out.onfinish = () => {
      gp.innerHTML = gameHTML(a);
      gp.scrollTop = 0;
      loadReadme(a);
      gp.animate([{ opacity: 0, transform: `translateX(${90 * dir}px)` }, { opacity: 1, transform: 'none' }], { duration: 520, easing: 'cubic-bezier(.3,1.2,.4,1)' });
    };
  }
  function backGame() {
    if (P.stack.length > 1) { P.stack.pop(); return swapGame(appOf(P.stack[P.stack.length - 1]), -1); }
    closeGame();
  }
  // The cover it came from, if that's still on screen to fly back to.
  function flyBackTarget() {
    if (!P.src || !P.src.layer || !P.src.layer.isConnected) return null;
    const node = [...P.src.layer.querySelectorAll('[data-open]:not([data-soft]) .ph-cover')].find((c) => c.closest('[data-open]').dataset.open === P.src.id);
    if (!node) return null;
    const r = rin(node);
    const off = r.y < 0 || r.y + r.h > el.ui.offsetHeight || r.x < -10 || r.x + r.w > el.ui.offsetWidth + 10;
    return off ? null : node;
  }
  function closeGame(slide) {
    if (!P.stack.length) return;
    const gp = el.gp, a = appOf(P.stack[0]);
    P.stack = [];
    depth();
    const src = slide ? null : flyBackTarget(), cover = $('#ph-gpcover', gp), pic = cover && $('img.ld', cover);
    if (src && pic && gp.scrollTop < 200) {
      const from = rin(cover), to = rin(src);
      cover.style.visibility = 'hidden';
      src.style.visibility = 'hidden';
      fly(pic.currentSrc || pic.src, from, to, 16, () => { src.style.visibility = ''; });
    }
    if (slide) {
      gp.style.transition = 'transform 460ms var(--ease),border-radius 300ms';
      gp.style.transform = 'translateX(105%)';
      gp.style.borderRadius = '34px';
    } else {
      gp.style.transition = 'opacity 280ms ease,transform 460ms var(--ease)';
      gp.style.opacity = '0';
      gp.style.transform = 'translateY(40px) scale(.97)';
    }
    setTimeout(() => { if (!P.stack.length) { gp.classList.remove('on'); gp.style.cssText = ''; gp.innerHTML = ''; } }, 470);
    void a;
    refresh();
  }

  // ---------- pages inside a tab: a console, all consoles, the chart, the account ----------
  const subHead = (title, sub, dotKey) => `<div class="ph-ptop"><button type="button" class="ph-ibtn glass ph-press" data-act="subback" aria-label="${esc(t('Back'))}">${ic('back')}</button></div><header class="ph-sbh"><div class="ph-col ph-grow"><h1 class="ph-disp">${dotKey ? conDot(dotKey, true) : ''}${esc(title)}</h1>${sub ? `<small>${esc(sub)}</small>` : ''}</div></header>`;
  function accountHTML() {
    const acc = S.account || {}, user = acc.signedIn && acc.user ? acc.user : null;
    if (!user) {
      return subHead(t('Account'), t('Not signed in.')) + `<div class="ph-in"><div class="ph-padx"><div class="ph-panel ph-acct"><span class="ph-ibtn ph-avatar huge">${ic('user')}</span><b>${esc(t('Your RecompStore account'))}</b><span class="ph-mut" style="font-size:13px;line-height:1.45;padding-bottom:14px">${esc(t('Sign in to keep your saved games with your account, on every device.'))}</span></div>
        <button type="button" class="ph-btn pri wide ph-press" data-act="signin" style="margin-top:14px">${esc(t('Sign in'))}</button>
        <button type="button" class="ph-btn sec wide ph-press" data-act="signup" style="margin-top:10px">${esc(t('Create your account'))}</button></div></div>`;
    }
    const face = user.avatar ? img(user.avatar) : esc((user.username || '?').slice(0, 1).toUpperCase());
    const reviews = Array.isArray(S.myReports) ? S.myReports.length : 0;
    const stats = [[Object.keys(S.library).length, t('games')], [Object.keys(saved()).length, t('saved')], [reviews, t('reviews')]];
    return subHead(t('Account'), t('Signed in')) + `<div class="ph-in"><div class="ph-padx"><div class="ph-panel ph-acct"><span class="ph-ibtn ph-avatar huge">${face}</span><b>${esc(user.username)}</b><div class="ph-stats">${stats.map(([v, k]) => `<div><b>${fmtNum(v)}</b><span>${esc(k)}</span></div>`).join('')}</div></div>
      <button type="button" class="ph-btn sec wide ph-press" data-act="account-site" style="margin-top:14px">${ic('globe', 's')}${esc(t('Manage account on the website'))}</button>
      <button type="button" class="ph-btn wide red ph-press" data-act="signout" style="margin-top:4px">${esc(t('Sign out'))}</button></div></div>`;
  }
  function subHTML(s) {
    if (s.t === 'con') {
      const c = consoles().find((x) => x.k === s.k) || { k: s.k, name: originName(s.k), list: [] };
      return `<span class="ph-spglow" style="--c:${conColor(c.k)}"></span>${subHead(c.name, tn(c.list.length, '1 game', '{n} games'), c.k)}<div class="ph-in"><div class="ph-grid long" style="padding-top:8px;padding-bottom:40px">${c.list.map((a) => card(a)).join('')}</div></div>`;
    }
    if (s.t === 'cons') {
      const cons = consoles();
      return subHead(t('Consoles'), tn(cons.length, '1 console', '{n} consoles')) + `<div class="ph-in"><div class="ph-cons list">${cons.map((c) => conTile(c, true)).join('')}</div></div>`;
    }
    if (s.t === 'account') return accountHTML();
    const top = ranked();
    return subHead(t('Trending this week'), t('Ranked by players')) + `<div class="ph-in"><div style="padding:2px 0 40px">${top.map((a, i) => lrow(a, { rank: i + 1 })).join('')}</div></div>`;
  }
  // All consoles: the row nearest the middle stands out, with a light tick as each one passes.
  function consFlow(tick) {
    const list = $('.ph-cons.list', el.sp);
    if (!list) return;
    const box = el.sp.getBoundingClientRect();
    let best = -1, bestD = 9;
    [...list.children].forEach((row0, i) => {
      const r = row0.getBoundingClientRect();
      const d = ((r.top + r.height / 2) - (box.top + box.height * 0.46)) / box.height;
      const far = Math.min(1, Math.abs(d) * 1.7);
      row0.style.transform = `scale(${(1 - far * 0.1).toFixed(3)})`;
      row0.style.opacity = (1 - far * 0.4).toFixed(2);
      if (Math.abs(d) < bestD) { bestD = Math.abs(d); best = i; }
    });
    if (tick && best !== consFlow.i) buzz('tick');
    consFlow.i = best;
  }
  // One page inside a tab pushes the next straight in (or pops back): a copy of the page that's
  // leaving slides away while the new one slides over, so the tab underneath never shows through.
  function subSwap(dir) {
    const sp = el.sp, s = P.subs[P.subs.length - 1], y = sp.scrollTop, ghost = sp.cloneNode(true), E = 'cubic-bezier(.22,1,.36,1)';
    ghost.removeAttribute('id');
    ghost.style.cssText = 'pointer-events:none;transform:none;z-index:' + (dir > 0 ? 30 : 31);
    if (dir > 0) sp.before(ghost); else sp.after(ghost);
    ghost.scrollTop = y;
    sp.innerHTML = subHTML(s);
    sp.scrollTop = dir > 0 ? 0 : (s.y || 0);
    requestAnimationFrame(() => consFlow());
    if (dir > 0) {
      sp.animate([{ transform: 'translateX(100%)' }, { transform: 'none' }], { duration: 540, easing: E });
      ghost.animate([{ transform: 'none', opacity: 1 }, { transform: 'translateX(-22%)', opacity: 0.4 }], { duration: 540, easing: E, fill: 'forwards' }).onfinish = () => ghost.remove();
      return;
    }
    sp.animate([{ transform: 'translateX(-22%)', opacity: 0.4 }, { transform: 'none', opacity: 1 }], { duration: 460, easing: E });
    ghost.animate([{ transform: 'none' }, { transform: 'translateX(105%)' }], { duration: 440, easing: E, fill: 'forwards' }).onfinish = () => ghost.remove();
  }
  function openSub(s) {
    const sp = el.sp;
    buzz();
    if (P.subs.length) {
      P.subs[P.subs.length - 1].y = sp.scrollTop;
      P.subs.push(s);
      return subSwap(1);
    }
    P.subs = [s];
    sp.innerHTML = subHTML(s);
    sp.classList.add('on');
    sp.style.cssText = 'transition:none;transform:translateX(100%)';
    sp.scrollTop = 0;
    void sp.offsetWidth;
    sp.style.transition = 'transform 560ms var(--soft)';
    sp.style.transform = 'none';
    depth();
    setTimeout(() => consFlow(), 580);
  }
  function closeSub(all) {
    const sp = el.sp;
    if (!P.subs.length) return;
    if (!all && P.subs.length > 1) { P.subs.pop(); return subSwap(-1); }
    P.subs = [];
    depth();
    sp.style.transition = 'transform 420ms var(--ease)';
    sp.style.transform = 'translateX(105%)';
    setTimeout(() => { if (!P.subs.length) { sp.classList.remove('on'); sp.innerHTML = ''; } }, 430);
  }

  // ---------- search ----------
  function searchResults(q) {
    if (!q.trim()) {
      const chips = consoles().map((c) => `<button type="button" class="ph-chip ph-press" data-con="${esc(c.k)}">${conDot(c.k, false)}${esc(c.name)}</button>`).join('');
      return `<h2>${esc(t('Jump to a console'))}</h2><div class="ph-chips">${chips}</div><h2>${esc(t('People are playing'))}</h2><div class="ph-in">${ranked().slice(0, 5).map((a) => lrow(a, { right: '' })).join('')}</div>`;
    }
    const hay = (a) => `${a.name} ${a.game || ''} ${devName(a)} ${conName(a)} ${GENRES[a.genre] || ''}`;
    const found = pool().filter((a) => sMatch(hay(a), q)).sort(byDl);
    if (!found.length) return `<div class="ph-in">${empty('search', esc(t('Nothing matches that. Try a game, a console or a developer.')))}</div>`;
    return `<p class="n">${esc(tn(found.length, '1 result', '{n} results'))}</p><div class="ph-in">${found.slice(0, 60).map((a) => lrow(a)).join('')}</div>`;
  }
  function openSearch() {
    const so = el.so;
    P.search = true;
    so.innerHTML = `<div class="ph-sobar"><label class="ph-sfield live ph-grow">${ic('search')}<input id="ph-q" type="search" placeholder="${esc(t('Search games, consoles, developers'))}" autocomplete="off" enterkeyhint="search"></label><button type="button" data-act="search-close">${esc(t('Cancel'))}</button></div><div class="ph-sores" id="ph-sores">${searchResults('')}</div>`;
    so.classList.add('on');
    so.scrollTop = 0;
    setTimeout(() => { const q = $('#ph-q', so); if (q && P.search) q.focus({ preventScroll: true }); }, 260);
    buzz();
  }
  function closeSearch() {
    const q = $('#ph-q', el.so);
    P.search = false;
    el.so.classList.remove('on');
    if (q) q.blur();
  }

  // ---------- sheets ----------
  function sheet(html) {
    el.sheet.innerHTML = '<div class="ph-grab"></div>' + html;
    el.sheet.style.transform = '';
    el.sheet.scrollTop = 0;
    el.scrim.classList.add('on');
    P.sheet = true;
    requestAnimationFrame(() => el.sheet.classList.add('on'));
  }
  function closeSheet() {
    P.sheet = false;
    el.sheet.classList.remove('on', 'drag');
    el.sheet.style.transform = '';
    el.scrim.classList.remove('on');
  }
  const sheetHead = (a) => `<div class="ph-shead"><span class="thumb">${img(coverUrl(a, 160))}</span><div class="ph-col ph-grow"><b>${esc(a.name)}</b><small>${esc([a.version ? 'v' + String(verText(a.version)).replace(/^v/i, '') : '', sizeOf(a)].filter(Boolean).join(' · '))}</small></div></div>`;
  // A game's quick menu: from its page's dots, or a long press on its cover anywhere.
  function menuSheet(id) {
    const a = appOf(id);
    if (!a) return;
    const st = stateOf(a), eid = esc(a.id);
    const item = (icon, label, attr, cls = '') => `<button type="button" class="ph-step ph-press ${cls}" data-close="1" ${attr}>${ic(icon, 's')}<b>${esc(label)}</b></button>`;
    const rows = [];
    if (st === 'installed' || st === 'running') rows.push(item('play', t('Play'), `data-act="play" data-id="${eid}"`));
    if (st === 'update') rows.push(item('downloads', t('Update'), `data-act="update" data-id="${eid}"`), item('play', t('Play'), `data-act="play" data-id="${eid}"`));
    if (st === 'get') rows.push(item('downloads', t('Install'), `data-act="install" data-id="${eid}"`));
    if (!P.stack.includes(a.id)) rows.push(item('chev', t('Details'), `data-open="${eid}"`));
    if (!S.library[a.id] && !a.custom) rows.push(item('bookmark', isSaved(a.id) ? t('Remove from saved') : t('Save for later'), `data-act="save" data-id="${eid}"`, isSaved(a.id) ? 'on' : ''));
    if (a.repo && !a.local) rows.push(item('globe', t('Open the project page'), `data-act="project" data-id="${eid}"`));
    if (S.library[a.id] && st !== 'busy' && st !== 'uninstalling') rows.push(item('trash', t('Uninstall'), `data-act="uninstall" data-id="${eid}"`, 'red'));
    sheet(sheetHead(a) + rows.join(''));
  }
  // The README, asked for as the game's page opens (the launcher keeps a copy for a day).
  function loadReadme(a) {
    if (!a.repo || a.local || S.readme[a.repo] !== undefined) return;
    S.readme[a.repo] = 'loading';
    const done = (text) => { S.readme[a.repo] = text || null; if (P.readme === a.id) readmeSheet(a.id); };
    invoke('repo_readme', { repo: a.repo }).then(done).catch(() => done(null));
  }
  function readmeSheet(id) {
    const a = appOf(id);
    if (!a) return;
    loadReadme(a);
    const r = S.readme[a.repo];
    P.readme = id;
    const body = r === 'loading' || r === undefined ? `<div class="ph-row" style="gap:12px;padding:26px 0"><span class="ph-spin"></span><span class="ph-mut">${esc(t('Loading…'))}</span></div>` : r ? `<div class="ph-md md">${md(r)}</div>` : `<p class="ph-mut" style="margin-top:12px;line-height:1.5">${esc(t('This project has no README to show.'))}</p>`;
    sheet(`<p class="ph-eyebrow">${esc(['README', devName(a)].filter(Boolean).join(' · '))}</p><h2 class="ph-disp">${esc(a.name)}</h2>${body}<button type="button" class="ph-btn sec wide ph-press" data-close="1" data-act="project" data-id="${esc(a.id)}" style="margin-top:18px">${ic('globe', 's')}${esc(t('Open the project page'))}</button>`);
  }
  function langSheet() {
    const cur = ui().lang || 'auto';
    const item = (code, name) => `<button type="button" class="ph-step ph-press${code === cur ? ' on' : ''}" data-close="1" data-lang="${esc(code)}"><b class="ph-grow">${esc(name)}</b>${code === cur ? ic('check', 's') : ''}</button>`;
    sheet(`<p class="ph-eyebrow">${esc(t('Language'))}</p>${item('auto', t('Same as your phone'))}${LANGS.map(([c, n]) => item(c, n)).join('')}`);
  }

  // ---------- the frame ----------
  const idx = () => Math.max(0, pref.tabs().indexOf(P.tab));
  const viewEl = (k) => document.getElementById('ph-v-' + k);
  // A page is patched in place once it has been drawn: covers stay and its place is kept.
  function paint(node, html) {
    if (!node) return;
    if (node.childNodes.length) morph(node, html); else node.innerHTML = html;
  }
  function drawView(k) {
    const v = viewEl(k);
    if (v) paint(v, (VIEW[k] || viewHome)());
  }
  function buildPager() {
    const tabs = pref.tabs();
    el.pager.innerHTML = tabs.map((k) => `<section class="ph-view" id="ph-v-${k}"></section>`).join('');
    if (!tabs.includes(P.tab)) P.tab = 'home';
    tabs.forEach(drawView);
    setPager(false);
  }
  function placeInd(at) {
    const b = $('button', el.nav), ind = $('.ph-ind', el.nav);
    if (!b || !ind) return;
    ind.style.transform = P.land ? `translateY(${at * b.offsetHeight}px)` : `translateX(${at * b.offsetWidth}px)`;
  }
  function setPager(anim) {
    el.pager.classList.toggle('anim', !!anim);
    el.pager.style.transform = `translateX(${-idx() * 100}%)`;
    placeInd(idx());
  }
  function drawNav() {
    el.nav.classList.toggle('nolab', !pref.labels());
    el.nav.innerHTML = '<span class="ph-ind"></span>' + pref.tabs().map((k) => `<button type="button" class="${P.tab === k ? 'on' : ''}" data-tab="${k}" aria-label="${esc(t(TABS[k]))}">${ic(k)}<span>${esc(t(TAB_SHORT[k] || TABS[k]))}</span></button>`).join('');
    placeInd(idx());
  }
  function go(k, anim = true) {
    // Downloads without a tab of its own lives in the Library.
    if (!pref.tabs().includes(k)) {
      if (k !== 'downloads') return;
      P.libi = 1;
      k = 'library';
      drawView('library');
    }
    const changed = P.tab !== k;
    P.tab = k;
    setPager(anim);
    for (const b of el.nav.querySelectorAll('button')) b.classList.toggle('on', b.dataset.tab === k);
    if (!changed) return;
    const v = viewEl(k);
    v.classList.remove('enter');
    void v.offsetWidth;
    v.classList.add('enter');
    buzz();
  }
  // The download in progress, floating above the menu.
  function drawMini() {
    const id = Object.keys(S.jobs)[0], a = id ? appOf(id) : null;
    if (!a) { el.mini.classList.remove('on'); return; }
    paint(el.mini, `<span class="pic">${img(coverUrl(a, 160))}<span class="ph-ringp">${ring(id, 40)}</span></span><span class="ph-col ph-grow"><b class="ph-gname">${esc(a.name)}</b><small data-pt="${esc(id)}">${esc(jobText(id))}</small></span>`);
    el.mini.classList.add('on');
  }
  // Everything on screen, brought up to date in place.
  function refresh() {
    if (!P.on) return;
    pref.tabs().forEach(drawView);
    const s = P.subs[P.subs.length - 1];
    if (s) { const y = el.sp.scrollTop; paint(el.sp, subHTML(s)); el.sp.scrollTop = y; consFlow(); }
    for (const box of root.querySelectorAll('[data-cta]')) { const a = appOf(box.dataset.cta); if (a) paint(box, ctaHTML(a)); }
    if (P.search) { const q = $('#ph-q', el.so); paint($('#ph-sores', el.so), searchResults(q ? q.value : '')); }
    drawMini();
    applyLook();
  }
  let later = 0;
  const soon = () => { if (!P.on) return; clearTimeout(later); later = setTimeout(refresh, 120); };
  // A download ticking: only its bars, rings and words change, nothing is drawn again.
  function tick(id) {
    const j = jobOf(id), p = pct(j), text = jobText(id);
    for (const n of root.querySelectorAll(`[data-pw="${CSS.escape(id)}"]`)) n.style.width = p + '%';
    for (const n of root.querySelectorAll(`[data-pt="${CSS.escape(id)}"]`)) n.textContent = text;
    for (const n of root.querySelectorAll(`[data-pr="${CSS.escape(id)}"]`)) n.setAttribute('stroke-dashoffset', n.dataset.c * (1 - p / 100));
  }

  // ---------- the screen: its size, its edges, which way it's held ----------
  let insets = { top: 0, right: 0, bottom: 0, left: 0 };
  // The design is drawn 433 wide on a phone held upright. A narrower phone scales it down to fit, a
  // tablet keeps it at full size and shows more of everything; S and L are a step either side.
  function fit() {
    const short = Math.min(innerWidth, innerHeight);
    const base = short <= 433 ? short / 433 : short < 720 ? 1 : Math.min(1.25, short / 720);
    P.z = base * SIZES[pref.size()];
    P.land = innerWidth > innerHeight;
    const px = (v) => (v / P.z).toFixed(2) + 'px';
    const side = Math.max(insets.left, insets.right);
    root.style.setProperty('--z', P.z.toFixed(4));
    root.style.setProperty('--pt', px(insets.top));
    root.style.setProperty('--sb', px(insets.bottom));
    root.style.setProperty('--pnl', px(insets.left));
    root.style.setProperty('--pnr', px(insets.right));
    root.classList.toggle('ph-land', P.land);
    // The menu goes down the side away from the camera.
    root.classList.toggle('ph-flip', P.land && insets.right > insets.left);
    root.classList.toggle('ph-cam', side > 0);
    // The launcher's own short messages sit above the menu.
    document.body.style.setProperty('--ph-toast', String(Math.round(insets.bottom + (P.land ? 20 : 104 * P.z))));
    if (el.pager) { setPager(false); drawNav(); }
  }
  async function syncInsets() {
    if (S.os !== 'android') return fit();
    try { insets = Object.assign({ top: 0, right: 0, bottom: 0, left: 0 }, await invoke('android_insets')); } catch (e) { /* the last known sizes stay */ }
    fit();
  }
  window.phoneInsets = () => { if (P.on) syncInsets(); };
  addEventListener('resize', () => { if (P.on) { fit(); syncInsets(); } });
  function applyLook() {
    const theme = pref.theme();
    if (root.dataset.theme === theme) return;
    root.dataset.theme = theme;
    if (S.os === 'android') invoke('android_edges', { phone: true, light: theme === 'light' }).catch(() => {});
  }

  // ---------- starting up ----------
  // The logo over a slow wall of covers, while the list comes in. Shown once, when the app opens.
  function boot() {
    const b = el.boot;
    b.hidden = false;
    b.innerHTML = `<div class="ph-wall"></div><div class="veil"></div><div class="glow"></div><div class="mid">${mark(112)}<p class="ph-disp w">RecompLauncher</p></div><div class="ld"><i></i></div>`;
    const t0 = performance.now();
    const wall = () => {
      const ids = pool().filter((a) => a.cover).sort(byDl).slice(0, 20);
      if (ids.length < 6 || $('.c', b)) return;
      let cols = '';
      for (let c = 0; c < 7; c++) {
        const set = [];
        for (let i = 0; i < 5; i++) set.push(ids[(c * 3 + i * 4) % ids.length]);
        const pics = set.concat(set).map((a) => `<img src="${esc(coverUrl(a, 160))}" alt="" class="ld" draggable="false">`).join('');
        cols += `<div class="c" style="animation-duration:${22 + (c % 3) * 5}s">${pics}</div>`;
      }
      const w = $('.ph-wall', b);
      w.innerHTML = cols;
      requestAnimationFrame(() => w.classList.add('on'));
    };
    // Gone once the games are in and the logo has had its moment; never kept waiting long offline.
    const check = () => {
      wall();
      const age = performance.now() - t0, ready = !!S.cat && (pool().length > 0 || !!S.listGate || S.offline);
      if ((ready && age > 1900) || age > 7000) return endBoot();
      setTimeout(check, 150);
    };
    check();
  }
  function endBoot() {
    const b = el.boot, v = viewEl(P.tab);
    P.booted = true;
    b.classList.add('out');
    if (v) { v.classList.remove('enter'); void v.offsetWidth; v.classList.add('enter'); }
    setTimeout(() => { b.hidden = true; b.innerHTML = ''; }, 700);
  }

  // ---------- taps ----------
  async function save(key, value) {
    await saveUi({ [key]: value }, true);
  }
  async function act(b) {
    const id = b.dataset.id;
    switch (b.dataset.act) {
      case 'back': return backGame();
      case 'subback': return closeSub();
      case 'search': return openSearch();
      case 'search-close': return closeSearch();
      case 'install': buzz(); await startInstall(id); return refresh();
      case 'update': buzz(); await updateGame(id); return refresh();
      case 'update-all': buzz(); for (const a of updatesList()) await updateGame(a.id); return refresh();
      case 'play': buzz('ok'); return playGame(id);
      case 'cancel': await cancelDownload(id); return soon();
      case 'resume': resumeDl(id); return refresh();
      case 'uninstall':
        try { await invoke('uninstall', { id, deleteFiles: false }); } catch (err) { return fail(err); }
        try { await refreshState(); } catch (err) { /* the next look */ }
        return refresh();
      case 'save': buzz(); if (isSaved(id)) await removeFromLibrary(id); else await addToLibrary(id); return refresh();
      case 'project': { const a = appOf(id); return a && a.repo ? invoke('open_url', { url: repoHome(a.repo) }).catch(fail) : null; }
      case 'libview': await save('phone_libview', pref.libview() === 'grid' ? 'list' : 'grid'); return drawView('library');
      case 'signin': return window.signIn && window.signIn(false);
      case 'signup': return window.signIn && window.signIn(true);
      case 'signout': if (typeof signOut === 'function') await signOut(); return refresh();
      case 'account-site': return invoke('open_url', { url: SITE + '/account' }).catch(fail);
      case 'discord': return invoke('open_url', { url: discordUrl() }).catch(fail);
      case 'site': return invoke('open_url', { url: SITE }).catch(fail);
      case 'lang': return langSheet();
      case 'tv': return toTV();
      case 'files': buzz(); try { await invoke('android_ask_files_access'); } catch (err) { fail(err); } return filesCheck();
      case 'copies': return copiesSheet(id);
      case 'restore': return restoreAsk(b);
      case 'restore-go':
        buzz('ok');
        closeSheet();
        try { await invoke('saves_restore', { id, name: b.dataset.name || null, v: b.dataset.v || null }); toast(t('That save is back in place.')); } catch (err) { fail(err); }
        S.savesAll = null;
        return refresh();
      case 'backup':
        buzz();
        b.disabled = true;
        try { const c = await invoke('saves_backup', { id }); toast(c ? t('Backed up.') : t('Nothing changed since the last copy.')); } catch (err) { fail(err); }
        S.savesAll = null;
        return refresh();
    }
  }
  // A switch flips at once under the finger; what it changes follows.
  async function flip(b) {
    const key = b.dataset.sw, on = b.getAttribute('aria-checked') !== 'true';
    if (key.startsWith('tab:')) {
      const k = key.slice(4), tabs = pref.tabs();
      if (!on && tabs.length <= 3) return toast(t('Keep at least 3 tabs.'));
      if (on && tabs.length >= 5) return toast(t('5 tabs is the most that fits.'));
      b.setAttribute('aria-checked', String(on));
      buzz();
      await save('phone_tabs', on ? [...tabs, k] : tabs.filter((x) => x !== k));
      const y = viewEl('settings').scrollTop;
      buildPager();
      drawNav();
      viewEl('settings').scrollTop = y;
      return;
    }
    b.setAttribute('aria-checked', String(on));
    buzz();
    if (key === 'labels') { await save('phone_labels', on); return drawNav(); }
    if (key === 'haptics') return save('phone_haptics', on);
    if (key === 'padtv') return save('pad_tv', on);
    if (key === 'hideai') { await saveSettings({ hide_ai: on }, true); return refresh(); }
    if (key === 'cloudall') { await setCloudAll(on); return refresh(); }
  }
  async function pick(b) {
    const box = b.parentNode, key = box.dataset.sg, i = Number(b.dataset.sgi);
    box.style.setProperty('--i', i);
    box.querySelectorAll('button').forEach((x, n) => x.classList.toggle('on', n === i));
    buzz();
    if (key === 'lib') {
      const lp = $('#ph-lp', root);
      P.libi = i;
      lp.classList.add('anim');
      lp.style.transform = `translateX(${-i * 100}%)`;
      return;
    }
    if (key === 'theme') {
      await saveUi({ theme: THEMES[i] === 'light' ? 'light' : 'dark', oled: THEMES[i] === 'oled' }, true);
      paintTheme();
      return applyLook();
    }
    if (key === 'size') { await save('phone_size', ['s', 'm', 'l'][i]); fit(); }
  }

  let dragged = false, pressed = false, hold = 0;
  root.addEventListener('click', (e) => {
    // A swipe or a long press already did its own thing: the tap that ends it does nothing.
    if (dragged || pressed) { dragged = false; pressed = false; e.preventDefault(); return; }
    if (e.target === el.scrim) return closeSheet();
    // A link in a README opens in the browser.
    const link = e.target.closest('.ph-md [data-url]');
    if (link) { e.preventDefault(); return invoke('open_url', { url: link.dataset.url }).catch(fail); }
    const b = e.target.closest('button');
    if (!b || !root.contains(b) || b.disabled) return;
    const d = b.dataset;
    if (d.close) closeSheet();
    if (d.act) return act(b);
    if (d.sw) return flip(b);
    if (d.sgi !== undefined) return pick(b);
    if (d.menu) return menuSheet(d.menu);
    if (d.readme) return readmeSheet(d.readme);
    if (d.lang) return setLanguage(d.lang).then(() => { buildPager(); drawNav(); });
    if (d.sub) {
      if (P.stack.length) closeGame(true);
      return openSub({ t: d.sub });
    }
    if (d.con) { closeSearch(); return openSub({ t: 'con', k: d.con }); }
    if (d.open) return openGame(d.open, b);
    if (b === el.mini) { if (P.stack.length) closeGame(true); closeSub(true); return go('downloads'); }
    if (d.tab) {
      if (P.stack.length) closeGame(true);
      closeSub(true);
      // The tab already showing: back to its top, as phone apps do.
      if (d.tab === P.tab) { const v = viewEl(P.tab); if (v) v.scrollTo({ top: 0, behavior: 'smooth' }); return; }
      return go(d.tab);
    }
  });
  root.addEventListener('input', (e) => {
    if (e.target.id === 'ph-q') $('#ph-sores', el.so).innerHTML = searchResults(e.target.value);
  });
  // A cover shows once it has loaded; one that can't be fetched leaves the game's name. A banner's
  // logo that can't be fetched leaves its written title.
  root.addEventListener('load', (e) => {
    if (e.target.tagName !== 'IMG') return;
    SEEN.add(e.target.getAttribute('src'));
    e.target.classList.add('ld');
  }, true);
  root.addEventListener('error', (e) => {
    const n = e.target;
    if (n.tagName !== 'IMG') return;
    if (n.hasAttribute('data-logo') && n.nextElementSibling) n.nextElementSibling.hidden = false;
    n.remove();
  }, true);
  // Scrolling: the banner's dots follow it, pictures drift a little slower than the page, and the
  // consoles list lifts the row in the middle. Once a frame at most.
  let frame = 0;
  root.addEventListener('scroll', (e) => {
    const n = e.target;
    if (frame || !n.classList) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      if (n.classList.contains('ph-hero')) {
        P.slide = Math.round(n.scrollLeft / n.clientWidth);
        n.parentNode.querySelectorAll('.ph-dots i').forEach((d, i) => d.classList.toggle('on', i === P.slide));
        return;
      }
      if (n.id === 'ph-v-home') {
        const y = Math.min(n.scrollTop, 500);
        for (const pic of n.querySelectorAll('.ph-hero img.bg')) pic.style.transform = `translateY(${y * 0.35}px) scale(${1 + y / 2400})`;
        return;
      }
      if (n === el.gp) {
        const pic = $('#ph-gphimg', n);
        if (!pic || pic.classList.contains('blur')) return;
        const y = Math.min(n.scrollTop, 400);
        pic.style.transform = `translateY(${y * 0.4}px) scale(${1 + y / 2000})`;
        return;
      }
      if (n === el.sp) consFlow(true);
    });
  }, true);

  // ---------- swipes: between tabs, back from the edge, a sheet pulled down, a long press ----------
  let G = null;
  root.addEventListener('pointerdown', (e) => {
    dragged = false; pressed = false;
    clearTimeout(hold);
    if (!P.on || e.target.closest('input')) return;
    const t0 = e.target, u = el.ui.getBoundingClientRect();
    G = { x: e.clientX, y: e.clientY, at: performance.now(), ax: null, hs: t0.closest('.ph-hs,.ph-hero'), sheet: t0.closest('.ph-sheet'), layer: t0.closest('.ph-gp,.ph-sp'), inPager: !!t0.closest('.ph-pager'), inLib: !!t0.closest('.ph-lp') };
    if (G.layer) G.edge = (e.clientX - u.left) / P.z - parseFloat(getComputedStyle(root).getPropertyValue('--pnl')) < 32;
    const c = t0.closest('[data-open]');
    if (c && !G.edge && !t0.closest('.ph-sheet')) hold = setTimeout(() => { pressed = true; buzz(); menuSheet(c.dataset.open); }, 520);
  });
  addEventListener('pointermove', (e) => {
    if (!G) return;
    const dx = (e.clientX - G.x) / P.z, dy = (e.clientY - G.y) / P.z;
    if (!G.ax) {
      if (Math.hypot(dx, dy) < 7) return;
      clearTimeout(hold);
      const flat = Math.abs(dx) > Math.abs(dy);
      if (G.edge && flat && dx > 0) { G.ax = 'back'; G.layer.style.transition = 'none'; el.edge.style.transition = 'none'; }
      else if (G.sheet && !flat && dy > 0 && G.sheet.scrollTop <= 0) { G.ax = 'sheet'; G.sheet.classList.add('drag'); }
      else if (flat && !G.hs && G.inPager && !P.sheet) {
        const to = P.libi - Math.sign(dx);
        // Inside the Library, a swipe moves between its panes first, then on to the next tab.
        if (G.inLib && to >= 0 && to <= 2) { G.ax = 'lib'; $('#ph-lp', root).classList.remove('anim'); }
        else { G.ax = 'pager'; el.pager.classList.remove('anim'); el.nav.classList.add('drag'); }
      } else G.ax = 'none';
      dragged = G.ax !== 'none';
    }
    const W = el.ui.offsetWidth, n = pref.tabs().length;
    if (G.ax === 'pager') {
      const i = idx(), atEnd = (i === 0 && dx > 0) || (i === n - 1 && dx < 0), off = atEnd ? dx * 0.28 : dx, pw = el.pager.clientWidth;
      G.off = off;
      el.pager.style.transform = `translateX(${-i * pw + off}px)`;
      placeInd(Math.max(-0.15, Math.min(n - 0.85, i - off / pw)));
    } else if (G.ax === 'lib') {
      const lp = $('#ph-lp', root);
      G.off = dx;
      lp.style.transform = `translateX(${-P.libi * lp.clientWidth + dx}px)`;
    } else if (G.ax === 'back') {
      const d = Math.max(0, dx);
      G.d = d;
      G.layer.style.transform = `translateX(${d * 0.6}px) scale(${1 - Math.min(d / W, 0.5) * 0.2})`;
      G.layer.style.borderRadius = Math.min(34, d / 3) + 'px';
      el.edge.style.opacity = Math.min(1, d / 60);
      el.edge.style.transform = `translateX(${Math.min(0, -40 + d * 0.5)}px) scale(${Math.min(1.15, 0.8 + d / 260)})`;
    } else if (G.ax === 'sheet') {
      G.d = Math.max(0, dy);
      G.sheet.style.transform = `translateY(${G.d}px)`;
    }
  }, { passive: true });
  function endSwipe() {
    clearTimeout(hold);
    if (!G) return;
    const g = G, fast = performance.now() - g.at < 260, n = pref.tabs().length;
    G = null;
    if (g.ax === 'pager') {
      const pw = el.pager.clientWidth, o = g.off || 0;
      let i = idx();
      el.nav.classList.remove('drag');
      if (Math.abs(o) > pw * 0.2 || (fast && Math.abs(o) > 30)) i = Math.max(0, Math.min(n - 1, i - Math.sign(o)));
      if (pref.tabs()[i] !== P.tab) go(pref.tabs()[i]); else setPager(true);
    }
    if (g.ax === 'lib') {
      const lp = $('#ph-lp', root), o = g.off || 0, box = $('[data-sg="lib"]', root);
      if (Math.abs(o) > lp.clientWidth * 0.2 || (fast && Math.abs(o) > 30)) P.libi = Math.max(0, Math.min(2, P.libi - Math.sign(o)));
      lp.classList.add('anim');
      lp.style.transform = `translateX(${-P.libi * 100}%)`;
      box.style.setProperty('--i', P.libi);
      box.querySelectorAll('button').forEach((b, k) => b.classList.toggle('on', k === P.libi));
      buzz();
    }
    if (g.ax === 'back') {
      const far = (g.d || 0) > 90 || (fast && g.d > 40);
      el.edge.style.transition = 'opacity 200ms,transform 200ms';
      el.edge.style.opacity = 0;
      el.edge.style.transform = 'translateX(-100%)';
      if (!far) {
        g.layer.style.transition = 'transform 460ms var(--spring),border-radius 200ms';
        g.layer.style.transform = 'none';
        g.layer.style.borderRadius = '0';
        return;
      }
      buzz();
      const deeper = g.layer === el.gp ? P.stack.length > 1 : P.subs.length > 1;
      if (deeper) {
        g.layer.style.transition = 'transform 300ms var(--soft),border-radius 200ms';
        g.layer.style.transform = 'none';
        g.layer.style.borderRadius = '0';
      }
      if (g.layer !== el.gp) return closeSub();
      if (deeper) return backGame();
      closeGame(true);
    }
    if (g.ax === 'sheet') {
      g.sheet.classList.remove('drag');
      if ((g.d || 0) > 80 || (fast && g.d > 30)) closeSheet(); else g.sheet.style.transform = '';
    }
  }
  addEventListener('pointerup', endSwipe);
  addEventListener('pointercancel', endSwipe);

  // Android's Back: the nearest thing closes, then Home, then out of the app (false).
  function back() {
    if (P.sheet) { closeSheet(); return true; }
    if (P.search) { closeSearch(); return true; }
    if (P.stack.length) { backGame(); return true; }
    if (P.subs.length) { closeSub(); return true; }
    if (P.tab !== 'home') { go('home'); return true; }
    return false;
  }

  // ---------- in and out ----------
  function enter() {
    if (P.on) return;
    P.on = true;
    document.body.classList.add('phone-on');
    root.hidden = false;
    root.innerHTML = `<div class="ph-ui"><div class="ph-base"><div class="ph-pager"></div></div>
      <button type="button" class="ph-mini ph-glassy ph-press"></button><nav class="ph-nav ph-glassy" aria-label="${esc(t('Sections'))}"></nav>
      <div class="ph-sp"></div><div class="ph-so"></div><div class="ph-gp"></div>
      <div class="ph-edgehint">${ic('back', 's')}</div><div class="ph-scrim"></div><div class="ph-sheet"></div>
      <div class="ph-safe-t"></div><div class="ph-safe-s"></div><div class="ph-boot" hidden></div></div>`;
    const q = (s) => $(s, root);
    el = { ui: q('.ph-ui'), pager: q('.ph-pager'), nav: q('.ph-nav'), mini: q('.ph-mini'), sp: q('.ph-sp'), so: q('.ph-so'), gp: q('.ph-gp'), edge: q('.ph-edgehint'), scrim: q('.ph-scrim'), sheet: q('.ph-sheet'), boot: q('.ph-boot') };
    Object.assign(P, { stack: [], subs: [], search: false, sheet: false, src: null });
    invoke('android_orientation', { landscape: false }).catch(() => {});
    applyLook();
    fit();
    syncInsets();
    if (window.hookAndroidBack) window.hookAndroidBack();
    buildPager();
    drawNav();
    drawMini();
    filesCheck();
    if (!P.booted) boot();
  }
  function leave() {
    P.on = false;
    document.body.classList.remove('phone-on');
    document.body.style.removeProperty('--ph-toast');
    root.hidden = true;
    root.innerHTML = '';
    delete root.dataset.theme;
    el = {};
    // Big Picture mode is full screen and keeps clear of the camera hole.
    if (S.os === 'android') invoke('android_edges', { phone: false, light: false }).catch(() => {});
  }
  // Big Picture mode, for a controller (or picked in Settings). It closes this layout as it opens.
  const toTV = () => window.startTV && window.startTV();
  window.startPhone = enter;
  window.phoneLeave = leave;
  window.phoneOn = () => P.on;
  window.phoneBack = back;
  // Whether a controller's button press should switch to Big Picture mode (Settings, here).
  window.phonePadTv = () => pref.padTv();

  // Keep the pages current while things happen elsewhere.
  const phases = {};
  T.event.listen('install', ({ payload: p }) => {
    if (!P.on || !p) return;
    const same = phases[p.id] === p.phase;
    phases[p.id] = p.phase;
    if (p.phase === 'done') { P.just = p.id; buzz('ok'); setTimeout(() => { P.just = ''; }, 2000); }
    // The launcher's own listener (app.js) updates the numbers first.
    if (same && p.phase === 'download') return setTimeout(() => tick(p.id), 0);
    soon();
  });
  T.event.listen('game', () => soon());
  // A copy made or sent after a game: the Saves tab reads the numbers again.
  for (const ev of ['saves', 'save-sync']) T.event.listen(ev, () => { S.savesAll = null; soon(); });
  // Back from Android's settings page: All files access may have just been allowed.
  document.addEventListener('visibilitychange', () => { if (!document.hidden && P.on) filesCheck(); });
  window.phoneRefresh = soon;
})();
