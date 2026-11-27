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
  // Display size: two steps smaller and two bigger than the design (S and L as they were).
  const SIZES = { xs: 0.8, s: 0.82 / 0.9, m: 1, l: 1 / 0.9, xl: 1.22 };
  const SIZE_KEYS = ['xs', 's', 'm', 'l', 'xl'];
  const SIZE_LABELS = ['XS', 'S', 'M', 'L', 'XL'];
  const THEMES = ['dark', 'oled', 'light'];
  const ui = () => (S.settings && S.settings.ui) || {};
  const pref = {
    tabs: () => { const t0 = (ui().phone_tabs || []).filter((k) => TABS[k]); return t0.length >= 3 ? Object.keys(TABS).filter((k) => t0.includes(k)) : TABS_FIRST; },
    labels: () => ui().phone_labels === true,
    haptics: () => ui().phone_haptics !== false,
    size: () => (SIZES[ui().phone_size] ? ui().phone_size : 'm'),
    // Big Picture has its own size, and its own look: Phone (made for this screen) or Full (the
    // big-screen one tablets and PCs have). Unset, the device decides (app.js droidPhone).
    bpSize: () => (SIZES[ui().phone_bp_size] ? ui().phone_bp_size : 'm'),
    bpStyle: () => (ui().bp_style === 'full' ? 'full' : ui().bp_style === 'phone' ? 'phone' : window.droidPhone && !droidPhone() ? 'full' : 'phone'),
    bold: () => ui().phone_bold === true,
    solid: () => ui().phone_solid === true,
    theme: () => (ui().theme === 'light' ? 'light' : ui().oled ? 'oled' : 'dark'),
    libview: () => (ui().phone_libview === 'list' ? 'list' : 'grid'),
    padTv: () => ui().pad_tv !== false,
    lite: () => ui().phone_lite === true,
    still: () => ui().phone_still === true,
    contrast: () => ui().phone_contrast === true,
  };
  // Optimization mode: flat (no blurs, glows or shadows) and no moving extras, for older phones.
  const lite = () => pref.lite();
  // Movement kept to the least: Optimization mode, Reduce motion, or the phone's own setting.
  const still = () => pref.lite() || pref.still() || (window.matchMedia && matchMedia('(prefers-reduced-motion: reduce)').matches);

  // P: where the player is. stack: game pages open (the last shows). subs: pages inside a tab.
  const ALL_STEP = 30;
  const P = { on: false, booted: false, tab: 'home', libi: 0, stack: [], subs: [], search: false, src: null, land: false, z: 1, allN: ALL_STEP };
  let el = {};
  // Big Picture mode, while it's open (its own section, further down).
  let bp = null;

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
    layers: '<path d="m12 3 9 5-9 5-9-5z"/><path d="m3 13 9 5 9-5"/>',
    bookmark: '<path d="M6 3h12v18l-6-4-6 4z"/>',
    trash: '<path d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3"/>',
    file: '<path d="M14 3H6v18h12V7zM14 3v4h4"/>',
    list: '<path d="M8 6h13M8 12h13M8 18h13M3.5 6h.01M3.5 12h.01M3.5 18h.01"/>',
    tv: '<rect x="2" y="5" width="20" height="13" rx="2.5"/><path d="M8 21h8"/>',
    globe: '<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18"/>',
    user: '<circle cx="12" cy="8" r="4"/><path d="M4 21a8 8 0 0 1 16 0"/>',
    chev: '<path d="m9 5 7 7-7 7"/>',
    chat: '<path d="M21 12a8 8 0 0 1-11.6 7.1L4 20l1-4.6A8 8 0 1 1 21 12z"/>',
    discord: '<path fill="currentColor" stroke="none" d="M19.3 5.4A16.6 16.6 0 0 0 15.2 4l-.5 1a15.4 15.4 0 0 0-5.4 0l-.5-1a16.5 16.5 0 0 0-4.1 1.4C2.1 9.3 1.4 13.1 1.8 16.9a16.7 16.7 0 0 0 5 2.6l1.1-1.7a10.8 10.8 0 0 1-1.7-.8l.4-.3a11.9 11.9 0 0 0 10.8 0l.4.3c-.5.3-1.1.6-1.7.8l1.1 1.7a16.6 16.6 0 0 0 5-2.6c.5-4.4-.8-8.2-2.9-11.5zM8.7 14.6c-1 0-1.8-.9-1.8-2s.8-2 1.8-2 1.8.9 1.8 2-.8 2-1.8 2zm6.6 0c-1 0-1.8-.9-1.8-2s.8-2 1.8-2 1.8.9 1.8 2-.8 2-1.8 2z"/>',
    lang: '<path d="M4 5h9M8.5 3v2M6 5c.6 3.4 3 6 6 7.5M11 5c-.6 3.6-3.4 6.5-7 8"/><path d="m12 21 4.5-10 4.5 10M13.8 17h5.4"/>',
    bug: '<path d="M9 7.5a3 3 0 0 1 6 0M8 10h8v4a4 4 0 0 1-8 0zM12 10v8M4 9l4 2M20 9l-4 2M4 19l4-2.5M20 19l-4-2.5M3.5 14H8M16 14h4.5"/>',
    undo: '<path d="M4 9h11a5 5 0 0 1 0 10H8M4 9l4-4M4 9l4 4"/>',
    spark: '<path d="M12 3v4M12 17v4M3 12h4M17 12h4M6 6l2.5 2.5M15.5 15.5 18 18M18 6l-2.5 2.5M8.5 15.5 6 18"/>',
    box: '<path d="M3 7.5 12 3l9 4.5v9L12 21l-9-4.5z"/><path d="M3 7.5 12 12l9-4.5M12 12v9"/>',
    shield: '<path d="M12 3 4.5 6v5.5c0 4.5 3.2 8.2 7.5 9.5 4.3-1.3 7.5-5 7.5-9.5V6z"/><path d="m8.5 12 2.5 2.5 4.5-5"/>',
    bell: '<path d="M6 16V11a6 6 0 0 1 12 0v5l1.5 2h-15zM10 20.5a2.2 2.2 0 0 0 4 0"/>',
    text: '<path d="M5 6V4.5h14V6M12 4.5v15M9 19.5h6"/>',
    eye: '<path d="M2.5 12S6 5 12 5s9.5 7 9.5 7-3.5 7-9.5 7-9.5-7-9.5-7z"/><circle cx="12" cy="12" r="3"/>',
    saves: '<path d="M15.2 3a2 2 0 0 1 1.4.6l3.8 3.8a2 2 0 0 1 .6 1.4V19a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2zM17 21v-7a1 1 0 0 0-1-1H8a1 1 0 0 0-1 1v7M7 3v4a1 1 0 0 0 1 1h7"/>',
    cloud: '<path d="M7 18a4.5 4.5 0 0 1-.6-9 6 6 0 0 1 11.5 1.6A3.8 3.8 0 0 1 17.5 18z"/>',
    up: '<path d="M12 20V9m0 0-4.5 4.5M12 9l4.5 4.5M5 4h14"/>',
    phone: '<rect x="6" y="2.5" width="12" height="19" rx="2.5"/><path d="M11 18.5h2"/>',
    key: '<circle cx="8" cy="15" r="4"/><path d="m11 12 9-9m-4 4 3 3"/>',
    refresh: '<path d="M20 11a8 8 0 0 0-14.3-4.9L4 8M4 4v4h4M4 13a8 8 0 0 0 14.3 4.9L20 16m0 4v-4h-4"/>',
    warn: '<path d="M10.3 4.3 2.9 17.4A2 2 0 0 0 4.6 20.4h14.8a2 2 0 0 0 1.7-3L13.7 4.3a2 2 0 0 0-3.4 0z"/><path d="M12 9.6v4.2M12 17.2h.01"/>',
    star: '<path d="M12 2.8l2.8 5.9 6.4.8-4.7 4.4 1.2 6.4L12 17.2 6.3 20.3l1.2-6.4-4.7-4.4 6.4-.8z"/>',
    trophy: '<path d="M8 21h8M12 17v4M7 4h10v5a5 5 0 0 1-10 0zM7 6H4a3 3 0 0 0 3 4M17 6h3a3 3 0 0 1-3 4"/>',
    pen: '<path d="M4 20h4L19 9l-4-4L4 16zM13.5 6.5l4 4"/>',
    notes: '<path d="M5 4h14v16H5zM9 8h6M9 12h6M9 16h3"/>',
    pad: '<path d="M6 11h4M8 9v4M15 12h.01M18 10h.01"/><path d="M17.3 5H6.7a4 4 0 0 0-4 3.6l-.9 7.2a2.6 2.6 0 0 0 2.6 2.9c1 0 1.7-.5 2.3-1.3L8 15.5h8l1.3 1.9c.6.8 1.3 1.3 2.3 1.3a2.6 2.6 0 0 0 2.6-2.9l-.9-7.2a4 4 0 0 0-4-3.6z"/>',
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
  const coverUrl = (a, css = 130) => (a && a.cover ? media(sharpOf(a.cover, css)) : '');
  // Art sized for how wide it shows, at this screen's sharpness (sharpOf). A backdrop fills the screen's long side when it's
  // cropped to fit, and a wide card is at most about 440 wide.
  const screenW = () => Math.max(window.innerWidth || 0, window.innerHeight || 0, 360);
  const cardW = () => Math.min(window.innerWidth || 440, 440);
  // Wide art, else the cover.
  const heroUrl = (a, css = screenW()) => (!a ? '' : a.hero ? media(sharpOf(a.hero, css)) : a.cover ? media(sharpOf(a.cover, Math.min(css, 300))) : '');
  // A page's backdrop: the wide art, or the cover blurred, where a small copy looks the same and decodes far quicker.
  const pageArt = (a) => heroUrl(a, a && a.hero ? screenW() : 60);
  // With Console Labels on Covers, the console's band across the top, as on a real game box.
  const coverBox = (a, cls = '', id = '', css = 130) => `<span class="ph-cover ${cls}"${id ? ` id="${id}"` : ''}><i>${esc(a.name)}</i>${img(coverUrl(a, css))}${coverLabel(a)}</span>`;

  // ---------- the games ----------
  // Each game once, as on the desktop: a game with several versions (the website's Versions) shows
  // the one you play, else the recommended one; its page lists the others. Worked out again only
  // when the catalog, the library or what's hidden changes.
  let poolMemo = null;
  const pool = () => {
    const u = ui(), k = { idx: index(), hid: hiddenSet(), ai: !!S.settings.hide_ai, lib: Object.keys(S.library).join('|'), look: [u.game_names, u.game_media, u.names, S.arts].map((x) => (typeof x === 'object' ? x : String(!!x))) };
    const m = poolMemo;
    if (m && m.idx === k.idx && m.hid === k.hid && m.ai === k.ai && m.lib === k.lib && m.look.every((x, i) => x === k.look[i])) return m.list;
    // Each through byId, so the names and pictures the player picked (Show Game Names and Media,
    // Rename, Change Cover) are the ones shown.
    poolMemo = Object.assign(k, { list: oneEach(visible().filter((a) => forOS(a) && !a.soon)).map((a) => byId(a.id) || a) });
    return poolMemo.list;
  };
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
  // This week's chart, always ten: the chart's games on Android first, then the most played and
  // downloaded (the chart is made from every system, so fewer of its games may be here).
  const TOP = 10;
  function ranked() {
    const all = pool(), chart = trendList(all).map((x) => x.a).slice(0, TOP);
    if (chart.length >= TOP) return chart;
    const have = new Set(chart.map((a) => a.id));
    return chart.concat([...all].sort((x, y) => (Number(y.players) || 0) - (Number(x.players) || 0) || byDl(x, y)).filter((a) => !have.has(a.id)).slice(0, TOP - chart.length));
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
    const one = (a) => `<button type="button" class="ph-wide ph-press" data-open="${esc(a.id)}" data-soft="1" data-key="w:${esc(a.id)}">${img(heroUrl(a, cardW()), 'bg')}<span class="ph-shade"></span><span class="foot"><span class="ph-col ph-grow"><span class="ph-gname">${esc(a.name)}</span><span class="ph-gsub">${esc(playedFor(S.library[a.id]))}</span></span><span class="ph-playdot">${ic('play', 's')}</span></span></button>`;
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
    return `<div class="ph-hhd" data-key="hd"><div>${mark(30)}<img class="ph-wm" src="img/${pref.theme() === 'light' ? 'wordmark-dark' : 'wordmark'}.svg" alt="RecompLauncher" draggable="false"><span class="ph-grow"></span>
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
    const pics = c.list.slice(0, wide ? 3 : 1).map((a) => img(coverUrl(a, 60))).join('');
    const count = wide ? `<small>${esc(tn(c.list.length, '1 game', '{n} games'))}</small>` : '';
    return `<button type="button" class="ph-con ph-press" data-con="${esc(c.k)}" data-key="c:${esc(c.k)}" style="--c:${conColor(c.k)}"><span class="glow"></span>${pics}<span class="k">${conDot(c.k, true)}<b>${esc(c.name)}</b>${count}</span></button>`;
  }
  function viewCatalog() {
    const all = pool(), cons = consoles(), top = ranked();
    const head = `<header class="ph-vhd" data-key="hd"><h1 class="ph-disp ph-vt">${esc(t('Catalog'))}</h1><p class="ph-vs">${esc(tf('{games} across {consoles}', { games: tn(all.length, '1 game', '{n} games'), consoles: tn(cons.length, '1 console', '{n} consoles') }))}</p></header>`;
    if (!all.length) return head + (gate() || empty('catalog', esc(S.offline ? t("Can't reach recomplauncher.com right now.") : t('No games yet'))));
    const az = [...all].sort((x, y) => x.name.localeCompare(y.name));
    // All games come in a few rows at a time as the page scrolls down (moreGames), not all at once.
    const shown = az.slice(0, P.allN);
    const tail = shown.length < az.length ? `<div class="ph-gridmore" data-key="allmore"><span class="ph-spin"></span></div>` : '';
    return `${head}
      <div class="ph-padx" data-key="find" style="margin-top:6px"><button type="button" class="ph-sfield ph-press" data-act="search">${ic('search')}<span>${esc(t('Search games, consoles, developers'))}</span></button></div>
      <section class="ph-sec" data-key="cons">${secHead(t('Consoles'), `<button type="button" data-sub="cons">${esc(t('See all'))}</button>`)}<div class="ph-cons">${cons.slice(0, 4).map((c) => conTile(c)).join('')}</div></section>
      <section class="ph-sec" data-key="trend">${secHead(t('Trending this week'), `<button type="button" data-sub="trend">${esc(tf('Top {n}', { n: top.length }))}</button>`)}${top.slice(0, 5).map((a, i) => lrow(a, { rank: i + 1 })).join('')}</section>
      <section class="ph-sec" data-key="all">${secHead(t('All games'))}<div class="ph-grid long">${shown.map((a) => card(a)).join('')}</div>${tail}</section>`;
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
    const tile = (a) => `<button type="button" class="ph-gcard ph-press" data-open="${esc(a.id)}" data-key="g:${esc(a.id)}"><span style="position:relative;display:block">${coverBox(a, isBusy(a) ? 'dim' : '', '', 180)}${libOver(a)}</span><span class="ph-gname">${esc(a.name)}</span><span class="ph-gsub">${libLine(a)}</span></button>`;
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
    try { const r = await invoke('android_files_access'); now = !!r.granted; if (r.device) P.model = r.device; } catch (e) { /* asked again next time */ }
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
    const stor = (a, i) => `<div class="ph-srow" data-key="st:${esc(a.id)}"><span class="thumb">${img(coverUrl(a, 60))}</span><div class="t"><b>${esc(a.name)}</b><span>${esc(sizeOf(a))}</span></div><button type="button" class="ph-ibtn ph-press ph-mut" data-act="uninstall" data-id="${esc(a.id)}" aria-label="${esc(tf('Uninstall {name}', { name: a.name }))}">${ic('trash', 's')}</button></div>`;
    return `<header class="ph-vhd" data-key="hd"><h1 class="ph-disp ph-vt">${esc(t('Settings'))}</h1></header>
      <button type="button" class="ph-panel ph-set link ph-press" data-sub="account" data-key="acc"><span class="ph-ibtn ph-avatar big">${face}</span><span class="ph-col ph-grow"><b>${esc(user ? user.username : t('Sign in'))}</b><small>${esc(user ? t('Signed in. Your saved games follow you.') : t('Keep your saved games with your account.'))}</small></span><span class="ph-mut">${ic('chev', 's')}</span></button>
      <div class="ph-setcols" data-key="cols">
      <div class="ph-panel ph-set" data-key="look"><h3>${esc(t('Appearance'))}</h3>
        ${srow(t('Theme'), t('OLED is pure black to save battery'), `<div class="ph-pick" style="width:188px">${seg('theme', [esc(tc('Dark|theme')), 'OLED', esc(tc('Light|theme'))], THEMES.indexOf(pref.theme()))}</div>`)}
        ${srow(t('Display size'), t('Bigger or smaller text, buttons and covers'), `<div class="ph-pick" style="width:190px">${seg('size', SIZE_LABELS, SIZE_KEYS.indexOf(pref.size()))}</div>`)}
        ${srow(t('Menu labels'), t('Show names under the menu icons'), sw('labels', pref.labels()))}
        ${srow(t('Vibrate on taps'), '', sw('haptics', pref.haptics()))}</div>
      <div class="ph-panel ph-set" data-key="covers"><h3>${esc(t('Covers and names'))}</h3>
        ${srow(t('Covers Only'), t('Hide the names under game covers.'), sw('bare', !!ui().hide_names))}
        ${srow(t('Console Labels on Covers'), t('A strip across the top of each cover with its console, like the band on a real game box.'), sw('cvl', !!ui().cover_labels))}
        ${srow(t('Show Game Names'), t('Name every recomp after its game, like Tears of the Kingdom instead of TotK Recomp. Names you gave games yourself stay.'), sw('gnames', !!ui().game_names))}
        ${srow(t('Show Game Media'), t('Every version of a game shows the same cover, background and logo: the recommended version\'s.'), sw('gmedia', !!ui().game_media))}</div>
      <div class="ph-panel ph-set" data-key="menu"><h3>${esc(t('Menu bar'))}</h3>
        <div class="ph-srow" style="min-height:0;padding:8px 0 4px"><span class="ph-mut" style="font-size:12.5px">${esc(tf('Pick 3 to 5 tabs. {n} chosen.', { n: tabs.length }))}</span></div>
        ${Object.keys(TABS).map(tabRow).join('')}</div>
      <div class="ph-panel ph-set" data-key="bp"><h3>${esc(t('Big Picture mode'))}</h3>
        ${srow(t('Big Picture style'), t('Phone is made for this screen. Full is the big-screen one tablets and PCs have.'), `<div class="ph-pick" style="width:150px">${seg('bpstyle', [esc(t('Phone')), esc(t('Full'))], pref.bpStyle() === 'full' ? 1 : 0)}</div>`)}
        ${pref.bpStyle() === 'phone' ? srow(t('Big Picture size'), t('Bigger or smaller text, buttons and covers in Big Picture'), `<div class="ph-pick" style="width:190px">${seg('bpsize', SIZE_LABELS, SIZE_KEYS.indexOf(pref.bpSize()))}</div>`) : ''}
        ${srow(t('Start in Big Picture mode'), t('Opens full screen, ready for a controller. Good for a TV or a handheld.'), sw('bpstart', !!S.settings.start_in_tv))}
        ${srow(t('Open with a controller'), t('Switches when you press a button on one'), sw('padtv', pref.padTv()))}
        <div class="ph-srow"><button type="button" class="ph-btn sec wide ph-press" data-act="tv">${ic('tv', 's')}${esc(t('Open Big Picture now'))}</button></div></div>
      <div class="ph-panel ph-set" data-key="cat"><h3>${esc(t('Catalog and updates'))}</h3>
        ${srow(t('Hide AI-assisted Recomps'), t('Recomps whose developers used AI tools for code, art, writing or voice won\'t show in the Catalog.'), sw('hideai', !!S.settings.hide_ai))}
        ${link('refresh', t('Refresh Catalog'), t('Get the newest list of recomps, their releases and their art now'), 'data-act="refresh-cat"')}
        ${link('downloads', t('Check for Game Updates'), updatesList().length ? tn(updatesList().length, '1 update ready', '{n} updates ready') : t('Looks for new releases of your games now'), 'data-act="check-upd"')}
        ${link('phone', t('Check for Launcher Updates'), tf('You have {v}', { v: S.version || '' }), 'data-act="self-upd"')}
        ${link('box', t('Bring Your Own Recomps'), t('Finds recomps already installed on this phone and adds them to your Library'), 'data-act="find-recomps"')}</div>
      <div class="ph-panel ph-set" data-key="rev"><h3>${esc(t('Reviews'))}</h3>
        ${srow(t('Ask After Playing'), t('After you play a game for a while, the launcher asks how it ran. Never again for a game you\'ve reviewed.'), sw('revask', window.reviewAskOn ? window.reviewAskOn() : true))}
        ${link('star', t('Your Reviews'), t('Every review you wrote, with any answer from its developer'), 'data-act="my-revs"')}</div>
      ${mine.length ? `<div class="ph-panel ph-set" data-key="stor"><h3>${esc(t('Storage'))}</h3><div class="ph-storh"><b>${esc(total ? fmtB(total) : '')}</b><span>${esc(tn(mine.length, '1 game', '{n} games'))}</span></div>
        ${total ? `<div class="ph-stor">${mine.map((a, i) => `<i style="flex:${(bytes[i] || 0) / total};--c:${conColor(a.origin)};animation-delay:${i * 60}ms"></i>`).join('')}</div>` : ''}${mine.map(stor).join('')}</div>` : ''}
      <div class="ph-panel ph-set" data-key="ease"><h3>${esc(t('Accessibility'))}</h3>
        ${srow(t('Bold text'), t('Heavier letters everywhere, easier to read'), sw('bold', pref.bold()))}
        ${srow(t('High contrast'), t('Stronger text and edges'), sw('contrast', pref.contrast()))}
        ${srow(t('Reduce transparency'), t('Solid menus and buttons instead of see-through ones'), sw('solid', pref.solid()))}
        ${srow(t('Reduce motion'), t('Pages change without sliding or zooming'), sw('still', pref.still() || pref.lite(), pref.lite()))}
        ${srow(t('Optimization mode'), t('Flat and still: no blurs, glows or moving extras. Smoother on older phones.'), sw('lite', pref.lite()))}</div>
      <div class="ph-panel ph-set" data-key="help"><h3>${esc(t('Help and privacy'))}</h3>
        ${link('bug', t('Report a Problem'), t('Something broke, an idea, or what you think so far. It goes straight to the developer.'), 'data-act="report"')}
        ${srow(t('Share usage and diagnostics'), t('Your phone\'s model, which games you play and for how long, and what went wrong when one won\'t install. No names, no files.'), sw('usage', !!S.settings.share_usage))}
        ${srow(t('Share anonymous usage stats'), t('When the launcher is open and which recomps are being played, under a random ID that isn\'t linked to you or your account.'), sw('stats', !!S.settings.share_stats))}</div>
      <div class="ph-panel ph-set" data-key="over"><h3>${esc(t('Start over'))}</h3>
        ${link('undo', t('Reset settings'), t('Every setting goes back to how it started. Your games and saves stay.'), 'data-act="reset-set"')}
        ${link('spark', t('Run first-time setup again'), t('Nothing is deleted.'), 'data-act="reset-setup"')}</div>
      <div class="ph-panel ph-set" data-key="about"><h3>${esc(t('About'))}</h3>
        ${link('lang', t('Language'), lang, 'data-act="lang"')}
        ${link('discord', 'Discord', t('Help, news and other players'), 'data-act="discord"')}
        ${link('globe', 'recomplauncher.com', '', 'data-act="site"')}
        ${srow(tf('RecompLauncher {v}', { v: S.version || '' }), engine(), '')}</div></div>`;
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
    const facts = [[t('Developer'), devName(a)], [t('Console'), conName(a)], [t('Genre'), GENRES[a.genre] ? t(GENRES[a.genre]) : ''], [t('Latest release'), a.released ? date(a.released) : '']].filter(([, v]) => v);
    const more = moreLike(a, 8);
    const based = a.game ? htmlFill(t('Based on {game}'), { game: esc(a.game) }) : '';
    const by = devName(a) ? htmlFill(t('by {who}'), { who: `<b>${esc(devName(a))}</b>` }) : '';
    return `<div class="ph-ptop"><button type="button" class="ph-ibtn glass ph-press" data-act="back" aria-label="${esc(t('Back'))}">${ic('back')}</button><button type="button" class="ph-ibtn glass ph-press" data-menu="${id}" aria-label="${esc(t('More options'))}">${ic('more', 's')}</button></div>
      <div class="ph-gph">${img(pageArt(a), wide ? '' : 'blur', 'id="ph-gphimg"')}<div class="tint"></div></div>
      <div class="ph-gpb">
        <div class="ph-gpt full">${coverBox(a, '', 'ph-gpcover', 180)}<div class="ph-col ph-grow"><div class="tags">${a.origin ? `<span class="ph-tag">${conDot(a.origin)}${esc(conName(a))}</span>` : ''}${GENRES[a.genre] ? `<span class="ph-tag">${esc(t(GENRES[a.genre]))}</span>` : ''}</div><h1 class="ph-disp" dir="auto">${esc(a.name)}</h1><p>${[based, by].filter(Boolean).join(' · ')}</p>${verPillPh(a)}</div></div>
        ${stats.length ? `<div class="ph-stats">${stats.map(([v, k]) => `<div><b class="${v.length > 7 ? 'long' : ''}">${esc(v)}</b><span>${esc(k)}</span></div>`).join('')}</div>` : ''}
        <div class="ph-cta" data-cta="${id}">${ctaHTML(a)}</div>
        ${a.soon ? `<p class="ph-mut" style="font-size:13px;line-height:1.45">${esc(soonNoteText(a))}</p>` : ''}
        ${playBox(a)}
        ${tags.length ? `<div class="ph-chips">${tags.map((x) => `<span class="ph-chip">${esc(x)}</span>`).join('')}</div>` : ''}
        ${gameAbout(a)}
        <div class="ph-revbox" data-rev="${id}">${revSection(a)}</div>
        ${pageRows(a)}
        ${facts.length ? `<div class="ph-panel ph-facts aside">${facts.map(([k, v]) => `<div><span>${esc(k)}</span><b>${esc(v)}</b></div>`).join('')}</div>` : ''}
        ${more.length ? `<div class="ph-more full">${secHead(t('More like this'))}<div class="ph-hs">${more.map((x) => card(x)).join('')}</div></div>` : ''}
      </div>`;
  }

  // Where a rectangle sits in the design's own pixels (the frame is scaled).
  // How many screen pixels a design pixel takes, as this web engine reports boxes (older engines report
  // a zoomed page's boxes in its own pixels, newer ones in the screen's).
  const zk = () => (el.ui.getBoundingClientRect().width / el.ui.offsetWidth) || 1;
  function rin(node) {
    const r = node.getBoundingClientRect(), u = el.ui.getBoundingClientRect(), k = zk();
    return { x: (r.left - u.left) / k, y: (r.top - u.top) / k, w: r.width / k, h: r.height / k };
  }
  function depth() {
    el.ui.classList.toggle('ph-deep', P.stack.length > 0);
    el.ui.classList.toggle('ph-side', P.subs.length > 0 && !P.stack.length);
  }
  // A cover flies from where it was tapped to its place on the game's page (and back).
  function fly(src, a, b, rad, done) {
    if (still()) { if (done) done(); return; }
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
  // Once a game's page fully covers the tabs, they're left out of layout and painting until it closes.
  let coverT = 0;
  function covered(on) {
    clearTimeout(coverT);
    if (!on) return el.pager.classList.remove('covered');
    coverT = setTimeout(() => { if (P.stack.length) el.pager.classList.add('covered'); }, 700);
  }
  function rise(gp) {
    covered(true);
    if (still()) { gp.style.opacity = '1'; gp.style.transform = 'none'; return; }
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
    // Where the tapped cover sits is read first, while the screen is still laid out, and the page is
    // built and laid out once after that.
    const soft = from && from.dataset.soft;
    const srcCover = from && !soft ? $('.ph-cover img.ld', from) : null;
    const from0 = srcCover ? rin(srcCover.parentNode) : null;
    gp.innerHTML = gameHTML(a);
    gp.classList.add('on');
    gp.style.cssText = 'transition:none;opacity:0;transform:none';
    depth();
    // A game's page always opens at its top (set once it's on screen: a hidden page can't scroll).
    gp.scrollTop = 0;
    loadReadme(a);
    const target = $('#ph-gpcover', gp);
    P.src = null;
    if (srcCover) {
      const srcBox = srcCover.parentNode;
      const to = rin(target);
      P.src = { layer: from.closest('.ph-view,.ph-sp,.ph-so'), id };
      target.style.visibility = 'hidden';
      srcBox.style.visibility = 'hidden';
      rise(gp);
      return fly(srcCover.currentSrc || srcCover.src, from0, to, 14, () => { target.style.visibility = ''; srcBox.style.visibility = ''; });
    }
    rise(gp);
    if (!still()) target.animate([{ transform: 'scale(.6)', opacity: 0 }, { transform: 'none', opacity: 1 }], { duration: 640, delay: 70, easing: 'cubic-bezier(.34,1.56,.64,1)', fill: 'backwards' });
  }
  // From one game's page to another (More like this), and back again.
  function swapGame(a, dir) {
    const gp = el.gp;
    if (still()) { gp.innerHTML = gameHTML(a); gp.scrollTop = 0; loadReadme(a); return; }
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
    covered(false);
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
    // The tab it uncovers is brought up to date now if it changed meanwhile; the rest wait until the
    // page has gone, so nothing heavy runs while it slides away.
    fresh(P.tab);
    setTimeout(() => { if (!P.stack.length) refresh(); }, 480);
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
    const mr = S.myReports, reviews = mr && Array.isArray(mr.list) ? mr.list.length : Array.isArray(mr) ? mr.length : 0;
    // Each number opens what it counts.
    const stats = [[Object.keys(S.library).length, t('games'), 'data-tab="library"'], [Object.keys(saved()).length, t('saved'), 'data-tab="library"'], [reviews, t('reviews'), 'data-act="my-revs"']];
    const used = acc.used || 0, quota = acc.quota || 150 * 1024 * 1024, pct = Math.min(100, Math.round((used / quota) * 100));
    const all = ui().cloud_all === true;
    const link = (icon, title, sub, attr) => `<button type="button" class="ph-srow ph-press" ${attr}>${ic(icon, 's')}<div class="t"><b>${esc(title)}</b>${sub ? `<span>${esc(sub)}</span>` : ''}</div><span class="ph-mut">${ic('chev', 's')}</span></button>`;
    const me = `<div class="ph-panel ph-acct"><span class="ph-ibtn ph-avatar huge">${face}</span><b>${esc(user.username)}</b><small class="ph-mut">${esc(acc.offline ? t('Signed in. The website can\'t be reached right now.') : t('Signed in on this phone'))}</small>
      <div class="ph-stats">${stats.map(([v, k, attr]) => `<button type="button" class="ph-press" ${attr}><b>${fmtNum(v)}</b><span>${esc(k)}</span></button>`).join('')}</div></div>`;
    const cloud = `<div class="ph-panel ph-set"><h3>${esc(t('Cloud saves'))}</h3>
      <div class="ph-space"><div class="ph-bar"><i style="width:${pct}%"></i></div><span>${esc(tf('{used} of {quota} used', { used: fmtB(used) || '0 KB', quota: fmtB(quota) }))}</span></div>
      <div class="ph-srow"><div class="t"><b>${esc(t('Back Up Every Game'))}</b><span>${esc(all ? t('On. Cloud saves are activated for all games. Turning it off disables cloud saves for every game. Saves stay intact.') : t('Off. Cloud saves are only active for the games you turn on in their Manage tab. Turning it on activates them for every game.'))}</span></div>${sw('cloudall', all)}</div>
      <div class="ph-srow ph-devname"><div class="t"><b>${esc(t('This Phone\'s Name'))}</b><span>${esc(t('Shown next to the saves this phone makes, so you can tell your devices apart. Leave it empty to use the phone\'s own name.'))}</span></div></div>
      <div class="ph-devin"><input id="ph-devname" type="text" maxlength="40" autocomplete="off" spellcheck="false" value="${esc(ui().device_name || '')}" placeholder="${esc(P.model || t('Phone name'))}" aria-label="${esc(t('This Phone\'s Name'))}"><button type="button" class="ph-btn sec ph-press" data-act="dev-save">${esc(t('Save'))}</button></div></div>`;
    const more = `<div class="ph-panel ph-set"><h3>${esc(t('Shortcuts'))}</h3>
      ${link('star', t('Your Reviews'), t('Every review you wrote, with any answer from its developer'), 'data-act="my-revs"')}
      ${link('saves', t('Saves'), t('Back them up, or restore an older one'), 'data-tab="saves"')}
      ${link('bug', t('Report a Problem'), t('Something broke, an idea, or what you think so far. It goes straight to the developer.'), 'data-act="report"')}
      ${link('globe', t('Manage account on the website'), t('Opens recomplauncher.com in your browser'), 'data-act="account-site"')}</div>`;
    return subHead(t('Account'), '') + `<div class="ph-in"><div class="ph-accgrid"><div class="ph-accside">${me}</div><div class="ph-accmain">${cloud}${more}</div><div class="ph-accout"><button type="button" class="ph-btn wide red ph-press" data-act="signout">${esc(t('Sign out'))}</button></div></div></div>`;
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
    const sp = el.sp, s = P.subs[P.subs.length - 1], y = sp.scrollTop, E = 'cubic-bezier(.22,1,.36,1)';
    if (still()) { sp.innerHTML = subHTML(s); sp.scrollTop = dir > 0 ? 0 : (s.y || 0); requestAnimationFrame(() => consFlow()); return; }
    const ghost = sp.cloneNode(true);
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
      // Consoles are one row here (their own page has them all), not a wall of chips.
      const cons = consoles();
      const browse = cons.length ? `<div class="ph-in"><button type="button" class="ph-panel ph-note ph-press ph-socons" data-act="search-cons"><span class="ib">${ic('library', 's')}</span><span class="ph-col ph-grow"><b>${esc(t('Consoles'))}</b><small>${esc(cons.slice(0, 4).map((c) => c.name).join(' · '))}</small></span><span class="ph-mut">${ic('chev', 's')}</span></button></div>` : '';
      return `${browse}<h2>${esc(t('People are playing'))}</h2><div class="ph-in">${ranked().slice(0, 6).map((a) => lrow(a, { right: '' })).join('')}</div>`;
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
    // In Big Picture mode a controller moves around the sheet, from its first button.
    if (bp) { bp.sf = ''; bpLight(false); }
  }
  function closeSheet() {
    P.sheet = false;
    P.rv = P.rvAll = null;
    el.sheet.classList.remove('on', 'drag', 'ph-rvsheet');
    if (bp && !bp.busy) bpLight(false);
    el.sheet.style.transform = '';
    el.scrim.classList.remove('on');
  }
  const sheetHead = (a) => `<div class="ph-shead"><span class="thumb">${img(coverUrl(a, 60))}</span><div class="ph-col ph-grow"><b>${esc(a.name)}</b><small>${esc([a.version ? 'v' + String(verText(a.version)).replace(/^v/i, '') : '', sizeOf(a)].filter(Boolean).join(' · '))}</small></div></div>`;
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
  // ---------- what players say: reviews, on a game's page and in Big Picture mode ----------
  // The same reviews as the desktop's (the website's, newest 60), each with its answer, where it was
  // played, the version, the player's words and any answer from the developer. Writing one needs an
  // account; names never show with them.
  const RV_SYS = { windows: 'Windows', linux: 'Linux', macos: 'Mac', deck: 'Steam Deck', android: 'Android' };
  const rvTone = (p) => (p >= 75 ? 'good' : p >= 45 ? 'mid' : 'low');
  const rvPill = (v) => `<span class="ph-rv v-${esc(v)}"><i></i>${esc(t(RUN_WORD[v] || ''))}</span>`;
  const reviewable = (a) => !!a && !a.soon && !a.custom && !(S.library[a.id] && S.library[a.id].local);
  function revLoad(a, force) {
    if (!reviewable(a) || (!force && S.reports[a.id] !== undefined)) return;
    loadReports(a.id, force).then(soon);
  }
  // Reviews with words in them first, newest first within each (the order they came in).
  const written = (list) => [...list.filter((x) => x.note && String(x.note).trim()), ...list.filter((x) => !(x.note && String(x.note).trim()))];
  function revItem(x) {
    const when = x.when ? ago(Date.parse(x.when) / 1000 || x.when) : '';
    const meta = [RV_SYS[x.sys] || '', x.version || '', when].filter(Boolean).join(' · ');
    const dev = (x.replies || []).filter((r) => r.dev).map((r) => `<div class="dev"><b>${esc(r.name || t('Developer'))}</b><span class="ph-tag">${esc(t('Developer'))}</span><p dir="auto">${esc(r.text)}</p></div>`).join('');
    return `<div class="ph-revi"><div class="top">${rvPill(x.verdict)}<small>${esc(meta)}</small></div>${x.note ? `<p dir="auto">${esc(x.note)}</p>` : ''}${x.specs ? `<p class="pc">${esc(x.specs)}</p>` : ''}${dev}</div>`;
  }
  // The score as an app store has it: this version's once three or more said, else every version's.
  function revScore(rep) {
    const sc = scoreOf(rep.counts, rep.latest), r = sc.r, p = r.pct('runs');
    const what = sc.cur ? tf('This version ({v})', { v: sc.v }) : t('say it plays great');
    const bar = ['runs', 'issues', 'broken'].map((k) => (r[k] ? `<i class="v-${k}" style="flex:${r[k]}"></i>` : '')).join('');
    return `<div class="ph-revs"><b class="${rvTone(p)}">${esc(tf('{pct}%', { pct: p }))}</b><div class="ph-col ph-grow"><span>${esc(what)}</span><small>${esc(tn(r.n, '1 review', '{n} reviews'))}</small></div></div><div class="ph-revbar">${bar}</div>`;
  }
  function revSay(a) {
    if (!S.library[a.id]) return '';
    const mine = myReviewOf(a.id);
    return `<button type="button" class="ph-btn sec ph-press" data-act="rev-say" data-id="${esc(a.id)}" data-key="rsay:${esc(a.id)}">${ic('pen', 's')}${esc(mine ? t('Edit Your Review') : t('Write a Review'))}</button>`;
  }
  function revSection(a) {
    if (!reviewable(a)) return '';
    const rep = S.reports[a.id];
    revLoad(a);
    const id = esc(a.id), say = revSay(a), mine = myReviewOf(a.id);
    const head = `<h2>${esc(t('Reviews'))}</h2>`;
    if (!rep || rep === 'loading') return `<div class="ph-panel ph-rev" data-key="rev:${id}" data-f>${head}<div class="ph-row" style="gap:10px"><span class="ph-spin"></span><span class="ph-mut">${esc(t('Loading…'))}</span></div></div>`;
    const all = runTally(rep.counts), items = written(rep.items || []);
    const own = mine ? `<div class="ph-revi mine"><div class="top"><b>${esc(t('Your Review'))}</b>${rvPill(mine.verdict)}<small>${esc(mine.version || '')}</small></div>${mine.note ? `<p dir="auto">${esc(mine.note)}</p>` : ''}</div>` : '';
    if (!all.n) {
      const why = rep.error ? t('Can\'t be reached right now') : S.library[a.id] ? t('Be the first: after you play, say how it ran.') : t('Nobody has said how it runs yet. Players who try it can say so from the launcher.');
      return `<div class="ph-panel ph-rev" data-key="rev:${id}" data-f>${head}${own}<p class="ph-mut">${esc(why)}</p>${say ? `<div class="ph-row acts">${say}</div>` : ''}</div>`;
    }
    const more = items.length > 3 ? `<button type="button" class="ph-btn sec ph-press" data-act="rev-all" data-id="${id}" data-key="rall:${id}">${esc(tf('All {n} reviews', { n: fmtNum(all.n) }))}</button>` : '';
    return `<div class="ph-panel ph-rev" data-key="rev:${id}" data-f>${head}${revScore(rep)}${own}${items.slice(0, 3).map(revItem).join('')}${say || more ? `<div class="ph-row acts">${say}${more}</div>` : ''}</div>`;
  }
  // Every review, narrowed to one answer by its chip.
  function revAllSheet(id, v) {
    const a = appOf(id), rep = S.reports[id];
    if (!a || !rep || rep === 'loading') return;
    P.rvAll = { id, v: v || '' };
    const items = written((rep.items || []).filter((x) => !P.rvAll.v || x.verdict === P.rvAll.v));
    const r = runTally(rep.counts);
    const chip = (k, label, n) => `<button type="button" class="ph-chip ph-press${P.rvAll.v === k ? ' on' : ''}" data-act="rev-v" data-id="${esc(id)}" data-v="${k}"${n || !k ? '' : ' disabled'}>${esc(label)}${k ? ` · ${fmtNum(n)}` : ''}</button>`;
    const chips = chip('', t('All'), r.n) + chip('runs', t('Plays great'), r.runs) + chip('issues', t('Has problems'), r.issues) + chip('broken', t('Won\'t run'), r.broken);
    const rest = r.n > (rep.items || []).length ? `<p class="ph-mut" style="font-size:12.5px">${esc(tf('The newest {n} are here. The rest are on recomplauncher.com.', { n: fmtNum((rep.items || []).length) }))}</p>` : '';
    sheet(`<p class="ph-eyebrow">${esc(tn(r.n, '1 review', '{n} reviews'))}</p><h2 class="ph-disp">${esc(a._name || a.name)}</h2>${revScore(rep)}<div class="ph-chips ph-rvchips">${chips}</div>
      <div class="ph-revl">${items.length ? items.map(revItem).join('') : `<p class="ph-mut">${esc(t('No reviews yet'))}</p>`}</div>${rest}
      ${revSay(a)}<button type="button" class="ph-btn sec wide ph-press" data-close="1" data-act="url" data-url="${esc(SITE + '/game/' + encodeURIComponent(id))}" style="margin-top:10px">${ic('globe', 's')}${esc(t('On recomplauncher.com'))}</button>`);
  }
  // "How did it run?": the three answers as big buttons, a few words (needed for a problem), and
  // what goes with it. Asked after a long session too, with Don't Ask Me Again. From more.js askRun.
  function reviewSheet(o) {
    const a = appOf(o.id) || { id: o.id, name: o.name };
    P.rv = { o, verdict: o.prev ? o.prev.verdict : '', busy: false, del: false };
    const pick = (k, n) => `<button type="button" class="ph-rvpick ph-press v-${k}" data-act="rv-v" data-v="${k}" role="radio" aria-checked="false"><span class="ic">${ic(k === 'runs' ? 'check' : k === 'issues' ? 'warn' : 'x')}</span><span class="ph-col"><b>${esc(t(n))}</b><small>${esc(t(window.VERDICT_SUB[k]))}</small></span></button>`;
    const when = o.secs >= 60 ? tf('{time} this session', { time: fmtTime(o.secs) }) : o.prev ? t('Edit Your Review') : t('Write a Review');
    const extra = [o.specsLine ? `${t('This goes with your review')}: ${o.specsLine}` : '', t('Your name isn\'t shown with your review.')].filter(Boolean);
    sheet(`<div class="ph-shead"><span class="thumb">${img(coverUrl(a, 60))}</span><div class="ph-col ph-grow"><small>${esc(when)}</small><b>${esc(tf('How did {name} run?', { name: o.name || a.name }))}</b></div></div>
      <div class="ph-rvpicks" role="radiogroup">${window.VERDICT.map(([k, n]) => pick(k, n)).join('')}</div>
      <div class="ph-rvw" hidden><b class="ph-rvq"></b><textarea class="ph-rvta" rows="4" maxlength="1000"></textarea><small class="ph-rvcount"></small></div>
      ${extra.map((x) => `<p class="ph-mut ph-rvnote">${esc(x)}</p>`).join('')}
      <p class="ph-rverr" hidden></p>
      <div class="ph-row ph-rvacts"><button type="button" class="ph-btn sec ph-press" data-act="rv-cancel">${esc(o.auto ? t('Not Now') : t('Cancel'))}</button><button type="button" class="ph-btn pri ph-press" data-act="rv-send" disabled>${esc(o.prev ? t('Update Review') : t('Send'))}</button></div>
      ${o.auto ? `<div class="ph-srow"><div class="t"><b>${esc(t('Don\'t Ask Me Again'))}</b><span>${esc(t('Turn it back on in Settings, Reviews.'))}</span></div>${sw('revnever', false)}</div>` : ''}
      ${o.prev ? `<button type="button" class="ph-btn sec wide ph-press red" data-act="rv-del">${ic('trash', 's')}${esc(t('Delete Review'))}</button>` : ''}`);
    el.sheet.classList.add('ph-rvsheet');
    if (o.prev && o.prev.note) $('.ph-rvta', el.sheet).value = o.prev.note;
    if (P.rv.verdict) rvPickIt(P.rv.verdict, true);
    return true;
  }
  const rvBad = (v) => !!v && v !== 'runs';
  function rvCheck() {
    const box = el.sheet, ta = $('.ph-rvta', box), go = $('[data-act="rv-send"]', box);
    if (!ta || !go || !P.rv) return;
    const v = P.rv.verdict, need = rvBad(v) ? Math.max(0, 15 - [...ta.value.trim()].length) : 0;
    go.disabled = !v || need > 0 || P.rv.busy;
    $('.ph-rvcount', box).textContent = !v ? t('Pick an answer first') : need ? tf('{n} more letters', { n: need }) : '';
  }
  function rvPickIt(v, quiet) {
    const box = el.sheet;
    P.rv.verdict = v;
    for (const p of box.querySelectorAll('.ph-rvpick')) { const on = p.dataset.v === v; p.classList.toggle('on', on); p.setAttribute('aria-checked', String(on)); }
    const w = $('.ph-rvw', box), ta = $('.ph-rvta', box);
    w.hidden = false;
    $('.ph-rvq', box).textContent = rvBad(v) ? t('What happened?') : t('Anything others should know?');
    ta.placeholder = rvBad(v) ? t('What happened, and when? (For example: crashes after the intro on the first level.)') : t('What works, what doesn\'t, settings that helped');
    rvCheck();
    // With fingers, the keyboard comes up for a problem's words; a controller stays on the buttons.
    if (!quiet && rvBad(v) && !(bp && bp.pad)) ta.focus();
  }
  async function rvSend() {
    const r = P.rv, box = el.sheet;
    if (!r || r.busy || !r.verdict) return;
    const note = $('.ph-rvta', box).value.trim();
    if (rvBad(r.verdict) && [...note].length < 15) return;
    r.busy = true;
    rvCheck();
    try {
      await r.o.send(r.verdict, note, false);
      buzz('ok');
      P.rv = null;
      closeSheet();
      delete S.reports[r.o.id];
      setTimeout(() => revLoad(appOf(r.o.id), true), 1500);
    } catch (e) {
      r.busy = false;
      const need = window.needSignIn(e);
      if (need != null) { closeSheet(); return phoneSignInAsk(need || t('Sign in to your RecompLauncher account to write a review. Your name isn\'t shown with it.')); }
      const err = $('.ph-rverr', box);
      err.textContent = String(e && e.message ? e.message : e);
      err.hidden = false;
      rvCheck();
    }
  }
  async function rvDelete(b) {
    const r = P.rv;
    if (!r || r.busy) return;
    // Asked once more, on the same button.
    if (!r.del) { r.del = true; b.lastChild.textContent = t('Delete your review? This can\'t be undone.'); return; }
    r.busy = true;
    try { await window.deleteReview(r.o.id); P.rv = null; closeSheet(); soon(); } catch (e) { r.busy = false; fail(e); }
  }
  function phoneSignInAsk(msg) {
    sheet(`<span class="ph-ei" style="margin:10px 0 6px">${ic('user')}</span><h2 class="ph-disp">${esc(t('Sign In to Write Reviews'))}</h2><p class="ph-mut" style="line-height:1.5;margin:8px 0 18px">${esc(msg)}</p>
      <div class="ph-row" style="gap:10px"><button type="button" class="ph-btn sec ph-press" data-close="1">${esc(t('Not Now'))}</button><button type="button" class="ph-btn pri ph-press" data-close="1" data-act="signin">${esc(t('Sign In'))}</button></div>`);
  }
  window.phoneReview = reviewSheet;
  window.phoneSignInAsk = phoneSignInAsk;
  // The versions of its game: another developer's take, a port, a build with extras. Opens one.
  const verPillPh = (a) => {
    const g = verGroup(a);
    return g ? `<button type="button" class="ph-verpill ph-press" data-act="vers" data-id="${esc(a.id)}" data-key="vers:${esc(a.id)}">${ic('layers', 's')}<span>${esc(tn(g.list.length, '1 version', '{n} versions'))}</span>${ic('chev', 's')}</button>` : '';
  };
  function versSheet(id) {
    const a = appOf(id), g = verGroup(a);
    if (!g) return;
    const row = (v) => {
      const tags = [v.id === g.pick ? `<span class="ph-tag acc">${esc(t('Recommended'))}</span>` : '', S.library[v.id] ? `<span class="ph-tag">${esc(t('Installed'))}</span>` : ''].join('');
      const line = [devName(v), v.adds || '', v.downloads ? `${fmtN(v.downloads)} ${t('downloads')}` : ''].filter(Boolean).join(' · ');
      return `<button type="button" class="ph-lmain ph-press ph-verrow${v.id === a.id ? ' on' : ''}" data-close="1" ${v.id === a.id ? '' : `data-open="${esc(v.id)}"`} data-key="v:${esc(v.id)}">${coverBox(v)}<span class="ph-col ph-grow"><span class="ph-gname">${esc(v.name)}</span><span class="ph-vtags">${tags}</span><span class="ph-gsub">${esc(line)}</span></span>${v.id === a.id ? ic('check', 's') : ic('chev', 's')}</button>`;
    };
    sheet(`<p class="ph-eyebrow">${esc(tn(g.list.length, '1 version', '{n} versions'))}</p><h2 class="ph-disp">${esc(g.name)}</h2>${g.why ? `<p class="ph-mut" style="font-size:13px;margin:4px 0 10px">${esc(g.why)}</p>` : ''}<div class="ph-verlist">${g.list.map(row).join('')}</div>`);
  }
  // What the developer wrote about it, and who helped make it.
  function notesSheet(id) {
    const a = appOf(id);
    if (!a) return;
    const body = [a.description ? `<div class="ph-md md">${md(a.description)}</div>` : '', a.credits ? `<h3 class="ph-sh3">${esc(t('Credits'))}</h3><div class="ph-md md">${md(a.credits)}</div>` : ''].join('');
    sheet(`<p class="ph-eyebrow">${esc(t('From the developer'))}</p><h2 class="ph-disp">${esc(a.name)}</h2>${body || `<p class="ph-mut">${esc(t('Nothing here yet.'))}</p>`}`);
  }
  // Its releases, newest first, each folded to its name until it's opened (the newest starts open),
  // as the desktop's Updates tab has them. One with an Android app in it can be installed from here:
  // that's how a player goes back to an older version when a new one broke something.
  const sameTag = (x, y) => String(x || '').trim().replace(/^v/i, '').toLowerCase() === String(y || '').trim().replace(/^v/i, '').toLowerCase();
  const wildRe = (pat) => new RegExp('^' + String(pat).replace(/[.+^${}()|[\]\\]/g, '\\$&').replace(/\*/g, '.*').replace(/\?/g, '.') + '$', 'i');
  const apkIn = (x, a) => { const pat = (a.assets || {}).android; const re = pat ? wildRe(pat) : null; return (x.assets || []).some((f) => /\.apk$/i.test(f.name || '') && (!re || re.test(f.name))); };
  function relsSheet(id) {
    const a = appOf(id);
    if (!a || !a.repo) return;
    P.rels = id;
    const r = S.rels[a.repo];
    if (!r) loadRels(a.repo).then(() => { if (P.rels === id && P.sheet) relsSheet(id); });
    const list = r && !r.loading ? (r.list || []).filter((x) => !x.draft).slice(0, 15) : null;
    const lib = S.library[id], droid = S.os === 'android';
    const row = (x, n) => {
      const latest = a.tag ? sameTag(x.tag_name, a.tag) : n === 0;
      const have = !!lib && !!lib.tag && sameTag(x.tag_name, lib.tag);
      const tags = [latest ? `<span class="ph-tag acc">${esc(t('Latest version'))}</span>` : '', have ? `<span class="ph-tag">${esc(t('Installed'))}</span>` : '', x.prerelease ? `<span class="ph-tag">${esc(t('Pre-release'))}</span>` : ''].join('');
      const can = !have && !a.local && (droid ? apkIn(x, a) : true);
      const pick = can ? `<button type="button" class="ph-btn sec wide ph-press" data-act="rel-pick" data-id="${esc(id)}" data-tag="${esc(x.tag_name)}">${ic('downloads', 's')}${esc(latest ? t('Install this version') : lib ? t('Go back to this version') : t('Install this version'))}</button>` : '';
      return `<div class="ph-relx${n ? '' : ' open'}"><button type="button" class="ph-relh ph-press" data-act="rel-t" aria-expanded="${n ? 'false' : 'true'}"><span class="ph-col ph-grow"><b dir="auto">${esc(x.name || x.tag_name)}</b><span class="ph-vtags">${tags}</span><small class="ph-mut">${esc([x.name && x.name !== x.tag_name ? x.tag_name : '', date(x.published_at)].filter(Boolean).join(' · '))}</small></span>${ic('chev', 's chv')}</button>
        <div class="ph-relb">${x.body ? `<div class="ph-md md">${md(x.body)}</div>` : `<p class="ph-mut">${esc(t('No notes for this one.'))}</p>`}${pick}</div></div>`;
    };
    const body = !list ? `<div class="ph-row" style="gap:12px;padding:26px 0"><span class="ph-spin"></span><span class="ph-mut">${esc(t('Loading the releases…'))}</span></div>`
      : !list.length ? `<p class="ph-mut">${esc(r.error ? t(r.error) : t('No releases yet.'))}</p>`
      : `<div class="ph-rels">${list.map(row).join('')}</div>`;
    sheet(`<p class="ph-eyebrow">${esc(t('Updates'))}</p><h2 class="ph-disp">${esc(a.name)}</h2>${body}`);
  }
  // A release picked from Updates. Newer than the one installed (or nothing installed): installed
  // over it. Older: Android won't put an older app over a newer one, so it's uninstalled first.
  function relPick(id, tag) {
    const a = appOf(id), lib = S.library[id], r = a && S.rels[a.repo];
    if (!a || !tag) return;
    buzz();
    const list = (r && r.list) || [];
    const at = (t0) => list.findIndex((x) => sameTag(x.tag_name, t0));
    const older = !!lib && !!lib.tag && at(tag) > at(lib.tag) && at(lib.tag) >= 0;
    if (S.os !== 'android' || !older) { closeSheet(); return startInstall(id, undefined, tag).then(refresh); }
    sheet(`<span class="ph-ei" style="margin:10px 0 6px">${ic('undo')}</span><h2 class="ph-disp">${esc(tf('Go back to {v}?', { v: tag }))}</h2>
      <p class="ph-mut" style="line-height:1.5;margin:8px 0 18px">${esc(t('Android only puts an older version on after the newer one is uninstalled. The game\'s own data on this phone goes with it, so back up its saves first if it keeps them in the app.'))}</p>
      <div class="ph-row" style="gap:10px"><button type="button" class="ph-btn sec ph-press" data-close="1">${esc(t('Cancel'))}</button><button type="button" class="ph-btn pri ph-press" data-close="1" data-act="down-go" data-id="${esc(id)}" data-tag="${esc(tag)}">${esc(t('Uninstall, then install'))}</button></div>`);
  }
  // Android's uninstall question first; once the player comes back with it gone, the older one installs.
  async function downGo(id, tag) {
    P.down = { id, tag, at: Date.now() };
    try { await invoke('uninstall', { id, deleteFiles: false }); } catch (e) { P.down = null; return fail(e); }
  }
  async function downBack() {
    const d = P.down;
    if (!d) return;
    P.down = null;
    await androidSync();
    try { await refreshState(); } catch (e) { /* the next look */ }
    if (S.library[d.id]) { refresh(); return toast(t('It\'s still installed, so nothing changed.')); }
    toast(tf('Installing {v}…', { v: d.tag }));
    await startInstall(d.id, undefined, d.tag);
    refresh();
  }


  // ---------- first-time setup, made for a phone ----------
  // No language (the phone's own) and no "how will you play" (fingers here, a controller opens Big
  // Picture): installing games, saves, recomps already on the phone, what's shared, the account.
  const FTS = ['hello', 'installs', 'saves', 'found', 'share', 'account', 'done'];
  function ftsOpen() {
    if (P.fts != null || bp) return;
    const box = document.createElement('div');
    box.className = 'ph-fts';
    el.ui.appendChild(box);
    el.fts = box;
    P.fts = 0;
    P.ftsInfo = { installs: null, files: null, found: null };
    ftsDraw();
  }
  async function ftsLook() {
    const i = P.ftsInfo;
    if (!i) return;
    try { i.installs = await invoke('android_install_allowed', { ask: false }); } catch (e) { i.installs = true; }
    try { i.files = !!(await invoke('android_files_access')).granted; } catch (e) { i.files = false; }
  }
  async function ftsDraw() {
    const box = el.fts;
    if (!box || P.fts == null) return;
    const k = FTS[P.fts], info = P.ftsInfo;
    if (k === 'installs' || k === 'saves') await ftsLook();
    if (k === 'found' && info.found == null) {
      const before = new Set(Object.keys(S.library));
      info.found = -1;
      androidSync().then(() => { info.found = Object.keys(S.library).filter((x) => !before.has(x)).length + before.size; if (FTS[P.fts] === 'found') ftsDraw(); });
    }
    if (el.fts !== box || FTS[P.fts] !== k) return;
    const ok = (on, yes, no) => `<p class="ph-ftsok${on ? ' on' : ''}">${ic(on ? 'check' : 'warn', 's')}${esc(on ? yes : no)}</p>`;
    const nav = (main, mainAct, skip) => `<div class="ph-ftsacts">${main ? `<button type="button" class="ph-btn pri wide ph-press" data-act="${mainAct}">${esc(main)}</button>` : ''}${skip ? `<button type="button" class="ph-btn sec wide ph-press" data-act="fts-next">${esc(skip)}</button>` : ''}</div>`;
    const swr = (key, title, sub, on) => `<div class="ph-srow"><div class="t"><b>${esc(title)}</b><span>${esc(sub)}</span></div>${sw(key, on)}</div>`;
    let body = '';
    if (k === 'hello') body = `<div class="ph-ftsmark">${mark(92)}</div><h1 class="ph-disp">${esc(t('Welcome to RecompLauncher'))}</h1><p>${esc(t('Recompiled games, running natively on your phone. A few things to get ready first. It takes a minute.'))}</p>${nav(t('Get started'), 'fts-next')}`;
    else if (k === 'installs') body = `<span class="ph-ei">${ic('shield')}</span><h1 class="ph-disp">${esc(t('Let RecompLauncher install games'))}</h1><p>${esc(t('Each game is its own app. Android asks once before RecompLauncher can install them: turn on Allow from this source, then come back here.'))}</p>${ok(info.installs, t('Allowed. Games install with one tap.'), t('Not allowed yet'))}${info.installs ? nav(t('Continue'), 'fts-next') : nav(t('Allow installing games'), 'perm-ask', t('Later'))}`;
    else if (k === 'saves') body = `<span class="ph-ei">${ic('saves')}</span><h1 class="ph-disp">${esc(t('Back up your saves'))}</h1><p>${esc(t('Some games keep their saves in your phone\'s shared folders. With file access, the launcher copies them after you play, and keeps them with your account when cloud saves are on.'))}</p>${ok(info.files, t('File access is on.'), t('File access is off'))}${info.files ? nav(t('Continue'), 'fts-next') : nav(t('Allow file access'), 'files', t('Later'))}`;
    else if (k === 'found') body = `<span class="ph-ei">${ic('box')}</span><h1 class="ph-disp">${esc(t('Games already on this phone'))}</h1><p>${esc(t('Recomps you installed some other way show up in your Library by themselves, and get updates like any other.'))}</p>${info.found < 0 ? `<p class="ph-ftsok"><span class="ph-spin s"></span>${esc(t('Looking…'))}</p>` : ok(info.found > 0, tn(info.found, '1 recomp is in your Library.', '{n} recomps are in your Library.'), t('None yet. Install one from the Catalog.'))}${nav(t('Continue'), 'fts-next')}`;
    else if (k === 'share') body = `<span class="ph-ei">${ic('bug')}</span><h1 class="ph-disp">${esc(t('Help improve RecompLauncher?'))}</h1><p>${esc(t('Both are off unless you turn them on, and you can change this any time in Settings.'))}</p>
      <div class="ph-panel ph-set">${swr('usage', t('Share usage and diagnostics'), t('Your phone\'s model, which games you play and for how long, and what went wrong when one won\'t install. No names, no files.'), !!S.settings.share_usage)}${swr('stats', t('Share anonymous usage stats'), t('When the launcher is open and which recomps are being played, under a random ID that isn\'t linked to you or your account.'), !!S.settings.share_stats)}</div>${nav(t('Continue'), 'fts-next')}`;
    else if (k === 'account') {
      const user = (S.account || {}).signedIn && S.account.user;
      body = `<span class="ph-ei">${ic('user')}</span><h1 class="ph-disp">${esc(user ? tf('Signed in as {name}', { name: user.username }) : t('Create an account'))}</h1><p>${esc(t('Your saved games and reviews follow you, and cloud saves keep your progress. It opens in your browser.'))}</p>${user ? nav(t('Continue'), 'fts-next') : nav(t('Sign in or create an account'), 'signin', t('Maybe later'))}`;
    } else body = `<span class="ph-ei">${ic('check')}</span><h1 class="ph-disp">${esc(t('You\'re all set'))}</h1><p>${esc(t('Everything here can be changed in Settings, and first-time setup can run again from there.'))}</p>${nav(t('Start playing'), 'fts-done')}`;
    const dots = FTS.map((_, n) => `<i class="${n === P.fts ? 'on' : n < P.fts ? 'done' : ''}"></i>`).join('');
    box.innerHTML = `<div class="ph-ftstop">${P.fts ? `<button type="button" class="ph-ibtn ph-press" data-act="fts-back" aria-label="${esc(t('Back'))}">${ic('back')}</button>` : '<span></span>'}<div class="ph-ftsdots">${dots}</div><span></span></div><div class="ph-ftsbody" data-step="${k}">${body}</div>`;
  }
  async function ftsDone() {
    buzz('ok');
    await saveSettings({ setup_done: true, data_asked: true }, true);
    const box = el.fts;
    P.fts = null;
    el.fts = null;
    if (!box) return;
    box.classList.add('out');
    setTimeout(() => box.remove(), 420);
    refresh();
  }
  // ---------- Settings' own pages ----------
  // The launcher's own update: Android installs the downloaded APK over this one.
  async function selfUpd() {
    buzz();
    toast(t('Checking for updates…'));
    let v = null, bad = false;
    try { v = await invoke('launcher_update', { force: true }); } catch (e) { bad = true; }
    if (!v) return toast(bad ? t('Couldn\'t check for updates right now.') : t('You have the newest RecompLauncher.'), bad);
    sheet(`<p class="ph-eyebrow">${esc(t('Update'))}</p><h2 class="ph-disp">${esc(tf('RecompLauncher {v}', { v: v.version || '' }))}</h2>${v.notes ? `<div class="ph-md md">${md(String(v.notes))}</div>` : ''}
      <p class="ph-mut" style="margin:12px 0;line-height:1.5">${esc(t('It downloads in your browser. Open the file when it\'s done and Android installs it over this one. Your games and settings stay.'))}</p>
      <button type="button" class="ph-btn pri wide ph-press" data-close="1" data-act="url" data-url="${esc(v.url || '')}">${ic('downloads', 's')}${esc(t('Download the update'))}</button>`);
  }
  // Recomps installed some other way (a download, another app) that the store lists: into the Library.
  async function findRecomps() {
    buzz();
    const before = new Set(Object.keys(S.library));
    await androidSync();
    const found = Object.keys(S.library).filter((k) => !before.has(k)).map(appOf).filter(Boolean);
    if (!found.length) return toast(t('No other recomps found on this phone. Ones the store lists show up here as soon as they\'re installed.'));
    sheet(`<p class="ph-eyebrow">${esc(t('Bring Your Own Recomps'))}</p><h2 class="ph-disp">${esc(tn(found.length, 'Found 1 recomp', 'Found {n} recomps'))}</h2><p class="ph-mut" style="margin:6px 0 10px">${esc(t('They\'re in your Library now, and get updates like any other.'))}</p>
      <div class="ph-verlist">${found.map((a) => `<button type="button" class="ph-lmain ph-press ph-verrow" data-close="1" data-open="${esc(a.id)}">${coverBox(a)}<span class="ph-col ph-grow"><span class="ph-gname">${esc(a.name)}</span><span class="ph-gsub">${esc(conName(a))}</span></span>${ic('chev', 's')}</button>`).join('')}</div>`);
    refresh();
  }
  // Your Reviews: every review the account wrote, newest answers first, each opening its game.
  function myRevsSheet() {
    const acc = S.account || {};
    if (!acc.signedIn) return phoneSignInAsk(t('Sign in to your RecompLauncher account to see your reviews.'));
    const m = S.myReports;
    if (!m || (!m.list && !m.error)) {
      if (!m || !m.loading) window.loadMyReports && window.loadMyReports();
      // Drawn again once they're in (for a little while: a slow answer is shown next time).
      const from = Date.now();
      P.myRevs = from;
      const again = () => { if (P.myRevs !== from || !P.sheet) return; const n = S.myReports; if (n && (n.list || n.error) && !n.loading) return myRevsSheet(); if (Date.now() - from < 15000) setTimeout(again, 500); };
      setTimeout(again, 500);
      return sheet(`<p class="ph-eyebrow">${esc(t('Your Reviews'))}</p><div class="ph-row" style="gap:12px;padding:26px 0"><span class="ph-spin"></span><span class="ph-mut">${esc(t('Loading…'))}</span></div>`);
    }
    P.myRevs = 0;
    const list = m.list || [];
    const item = (r) => {
      const a = appOf(r.app), name = (a && a.name) || r.name || r.app;
      const dev = (r.replies || []).filter((x) => x.dev).map((x) => `<div class="dev"><b>${esc(x.name || t('Developer'))}</b><span class="ph-tag">${esc(t('Developer'))}</span><p dir="auto">${esc(x.text)}</p></div>`).join('');
      const when = r.when ? ago(Date.parse(r.when) / 1000 || r.when) : '';
      return `<div class="ph-revi ph-myrev"><button type="button" class="ph-lmain ph-press" ${a ? `data-close="1" data-open="${esc(a.id)}"` : 'disabled'}>${a ? coverBox(a) : ''}<span class="ph-col ph-grow"><b class="ph-gname">${esc(name)}</b><span class="top">${rvPill(r.verdict)}<small>${esc([r.version || '', when].filter(Boolean).join(' · '))}</small></span></span></button>${r.note ? `<p dir="auto">${esc(r.note)}</p>` : ''}${dev}
        ${S.library[r.app] ? `<div class="ph-row acts"><button type="button" class="ph-btn sec sm ph-press" data-close="1" data-act="rev-say" data-id="${esc(r.app)}">${ic('pen', 's')}${esc(t('Edit'))}</button></div>` : ''}</div>`;
    };
    sheet(`<p class="ph-eyebrow">${esc(tn(list.length, '1 review', '{n} reviews'))}</p><h2 class="ph-disp">${esc(t('Your Reviews'))}</h2>
      ${m.error && !list.length ? `<p class="ph-mut">${esc(t('Your reviews couldn\'t be loaded'))}</p>` : !list.length ? `<p class="ph-mut" style="line-height:1.5">${esc(t('When you review a project, it shows here with any answer from its developer.'))}</p>` : `<div class="ph-revl">${list.map(item).join('')}</div>`}`);
  }
  // Report a Problem: what kind, which game (its log goes along), and a few words. Marked as from Android.
  const REP_KINDS = [['bug', N_('Something broke')], ['install', N_('Won\'t install')], ['idea', N_('An idea')], ['other', N_('Something else')]];
  function reportSheet(game) {
    P.rep = { kind: 'bug', game: game && S.library[game] ? game : '' };
    const r = P.rep;
    const mine = Object.keys(S.library).map(appOf).filter(Boolean);
    const chip = (act, k, label, on) => `<button type="button" class="ph-chip ph-press${on ? ' on' : ''}" data-act="${act}" ${act === 'rep-k' ? 'data-k' : 'data-g'}="${esc(k)}">${esc(label)}</button>`;
    sheet(`<span class="ph-ei" style="margin:10px 0 6px">${ic('bug')}</span><h2 class="ph-disp">${esc(t('Report a Problem'))}</h2><p class="ph-mut" style="line-height:1.5;margin:6px 0 14px">${esc(t('It goes straight to the developer. Pick the game if it\'s about one: its last log and how the launcher started it go along.'))}</p>
      <div class="ph-chips ph-repk">${REP_KINDS.map(([k, n]) => chip('rep-k', k, t(n), r.kind === k)).join('')}</div>
      ${mine.length ? `<b class="ph-reph">${esc(t('Which game?'))}</b><div class="ph-chips ph-repk">${chip('rep-g', '', t('Not about a game'), !r.game)}${mine.map((a) => chip('rep-g', a.id, a.name, r.game === a.id)).join('')}</div>` : ''}
      <b class="ph-reph">${esc(t('Tell us about it'))}</b><textarea class="ph-rvta ph-repta" rows="5" maxlength="1800" placeholder="${esc(t('What you did and what happened'))}"></textarea>
      <p class="ph-rverr" hidden></p>
      <div class="ph-row ph-rvacts"><button type="button" class="ph-btn sec ph-press" data-close="1">${esc(t('Cancel'))}</button><button type="button" class="ph-btn pri ph-press" data-act="rep-send">${esc(t('Send it'))}</button></div>`);
  }
  async function reportSend() {
    const r = P.rep, ta = el.sheet && $('.ph-repta', el.sheet), go = el.sheet && $('[data-act="rep-send"]', el.sheet), err = el.sheet && $('.ph-rverr', el.sheet);
    if (!r || !ta || r.busy) return;
    const text = ta.value.trim();
    if (text.replace(/\s/g, '').length < (r.kind === 'bug' ? 12 : 3)) { err.textContent = t('Tell us a little more: what you did and what happened.'); err.hidden = false; return ta.focus(); }
    const u = S.account && S.account.signedIn && S.account.user;
    r.busy = true;
    go.disabled = true;
    go.textContent = t('Sending…');
    try {
      await invoke('beta_feedback', { kind: r.kind, area: 'Android', mood: null, message: text, contact: u ? `account: ${u.username}` : null, attach: !!S.settings.share_usage, game: r.game || null });
      P.rep = null;
      closeSheet();
      buzz('ok');
      toast(t('Sent. Thanks for telling us.'));
    } catch (e) {
      r.busy = false;
      go.disabled = false;
      go.textContent = t('Send it');
      err.textContent = t(String(e && e.message ? e.message : e));
      err.hidden = false;
    }
  }
  // Reset settings, or first-time setup again: asked once.
  function resetAsk(setup) {
    sheet(`<span class="ph-ei" style="margin:10px 0 6px">${ic(setup ? 'spark' : 'undo')}</span><h2 class="ph-disp">${esc(setup ? t('Run first-time setup again?') : t('Reset settings?'))}</h2>
      <p class="ph-mut" style="line-height:1.5;margin:8px 0 18px">${esc(setup ? t('The launcher starts over in first-time setup. Your games, saves and settings stay as they are.') : t('Every setting goes back to how it started. Your games and saves stay.'))}</p>
      <div class="ph-row" style="gap:10px"><button type="button" class="ph-btn sec ph-press" data-close="1">${esc(t('Cancel'))}</button><button type="button" class="ph-btn pri ph-press" data-close="1" data-act="reset-go" data-k="${setup ? 'setup' : 'settings'}">${esc(setup ? t('Run it again') : t('Reset settings'))}</button></div>`);
  }
  async function resetGo(setup) {
    try {
      S.settings = await invoke('reset_settings', { what: setup ? 'setup' : 'settings' });
    } catch (e) { return fail(e); }
    if (setup) return location.reload();
    poolMemo = null;
    paintTheme();
    applyLook();
    fit();
    buildPager();
    drawNav();
    toast(t('Settings are back to how they started.'));
  }
  // Android installs games only once the launcher is allowed to: asked before anything downloads,
  // with Android's page a tap away, and the install carries on by itself once it's allowed.
  let permWait = null;
  function permSheet() {
    sheet(`<span class="ph-ei" style="margin:10px 0 6px">${ic('shield')}</span><h2 class="ph-disp">${esc(t('Allow installing games'))}</h2>
      <p class="ph-mut" style="line-height:1.5;margin:8px 0 18px">${esc(t('Android asks once before an app can install other apps. Turn on Allow from this source for RecompLauncher, then come back: the install carries on by itself.'))}</p>
      <div class="ph-row" style="gap:10px"><button type="button" class="ph-btn sec ph-press" data-close="1">${esc(t('Not Now'))}</button><button type="button" class="ph-btn pri ph-press" data-act="perm-ask">${esc(t('Open settings'))}</button></div>`);
    el.sheet.classList.add('ph-perm');
  }
  window.droidInstallGate = async (resume) => {
    let ok = true;
    try { ok = await invoke('android_install_allowed', { ask: false }); } catch (e) { return true; }
    if (ok) return true;
    permWait = resume;
    if (P.on && !document.body.classList.contains('tv-on')) permSheet();
    else { invoke('android_install_allowed', { ask: true }).catch(() => {}); toast(t('Allow RecompLauncher to install apps, then come back. The install carries on by itself.')); }
    return false;
  };
  window.droidInstallOk = () => invoke('android_install_allowed', { ask: false }).catch(() => true);
  document.addEventListener('visibilitychange', async () => {
    if (document.hidden) return;
    if (P.down) downBack();
    // Back from Android's settings or the browser during first-time setup: the step shows what changed.
    if (P.fts != null) setTimeout(ftsDraw, 300);
    if (!permWait) return;
    let ok = false;
    try { ok = await invoke('android_install_allowed', { ask: false }); } catch (e) { /* asked again next time */ }
    if (!ok) return;
    const go = permWait;
    permWait = null;
    if (P.sheet && el.sheet.classList.contains('ph-perm')) closeSheet();
    toast(t('Thanks. Installing now.'));
    go();
  });

  // A row on the game's page that opens one of those.
  const pageRow = (icon, title, sub, attr) => `<button type="button" class="ph-panel ph-note ph-press" ${attr}><span class="ib">${ic(icon, 's')}</span><span class="ph-col ph-grow"><b>${esc(title)}</b><small>${esc(sub)}</small></span><span class="ph-mut">${ic('chev', 's')}</span></button>`;
  function pageRows(a) {
    const id = esc(a.id);
    return [
      a.repo && !a.local ? pageRow('file', t('Read the README'), t('Notes, controls and known issues from the developer'), `data-readme="${id}" data-key="readme:${id}"`) : '',
      a.description || a.credits ? pageRow('notes', t('From the developer'), t('What it is, what it needs, and who made it'), `data-act="notes" data-id="${id}" data-key="notes:${id}"`) : '',
      a.repo && !a.local ? pageRow('up', t('Updates'), a.released ? tf('Latest release {when}', { when: date(a.released) }) : t('Every release and what changed'), `data-act="rels" data-id="${id}" data-key="rels:${id}"`) : '',
    ].join('');
  }
  // About the game itself (IGDB or LaunchBox): what it is and the facts, as the desktop has them.
  // Your play time on this phone, as the desktop's card has it (without its chart): all of it, this
  // week, this month, how many sessions and the longest. Only once it's been played.
  function playBox(a) {
    const g = S.library[a.id];
    if (!g || !g.last_played) return '';
    const { recent, count, longest } = playStats(a.id, g.sessions);
    const now = Date.now() / 1000, sum = (from) => recent.filter((x) => x[0] >= from).reduce((n, x) => n + x[1], 0);
    const nums = [
      [PT_IC.all, t('Played'), g.played_secs >= 60 ? fmtTimeShort(g.played_secs) : t('Under a minute')],
      [PT_IC.week, t('This week'), fmtTimeShort(sum(now - 7 * 86400))],
      [PT_IC.month, t('This month'), fmtTimeShort(sum(now - 30 * 86400))],
      [PT_IC.ses, t('Sessions'), fmtNum(count)],
      [PT_IC.long, t('Longest'), fmtTimeShort(longest)],
    ];
    return `<div class="ph-play" data-key="play:${esc(a.id)}">${nums.map(([icon, k, v]) => `<div><span>${icon}${esc(k)}</span><b>${esc(v)}</b></div>`).join('')}</div>`;
  }
  // About the original game, from IGDB as the desktop shows it (the whole summary in Big Picture, its
  // first part on a phone), or from Wikipedia when the catalog has nothing on it.
  function gameAbout(a, full) {
    const ab = a.about && (a.about.summary || aboutFacts(a.about).length) ? a.about : null;
    const own = [a.blurb || notesLine(a.description)].filter(Boolean);
    let wiki = null;
    if (!ab && !a.local && !a.custom) {
      if (S.info[a.id] === undefined) loadInfo(a.id).then(soon);
      wiki = S.info[a.id] && S.info[a.id] !== 'loading' && S.info[a.id].text ? S.info[a.id] : null;
    }
    if (!ab && !wiki && !own.length) return '';
    const facts = ab ? aboutFacts(ab).slice(0, full ? 9 : 6) : [];
    const src = ab ? aboutSource(ab) : wiki ? { label: t('From Wikipedia, under CC BY-SA.') } : null;
    const paras = (txt) => { const all = String(txt).split(/\n\s*\n/).map((x) => x.trim()).filter(Boolean); return full ? all : all.slice(0, 1); };
    const said = ab && ab.summary ? paras(ab.summary) : wiki ? paras(wiki.text) : [];
    const score = ab && ab.rating !== null && ab.rating !== '' && Number.isFinite(Number(ab.rating)) ? Math.round(Number(ab.rating)) : null;
    return `<div class="ph-about" data-key="about:${esc(a.id)}" data-f><h2>${esc(t('About'))}${score !== null ? `<span class="ph-score" title="${esc(t('Critic and player score'))}"><b>${score}</b>${esc(t('Critic and player score'))}</span>` : ''}</h2>${own.map((p) => `<p dir="auto">${esc(p)}</p>`).join('')}${said.map((p) => `<p dir="auto">${esc(p)}</p>`).join('')}
      ${facts.length ? `<div class="ph-panel ph-facts">${facts.map(([k, v]) => `<div><span>${esc(k)}</span><b dir="auto">${esc(v)}</b></div>`).join('')}</div>` : ''}${src ? `<p class="ph-src">${esc(src.label)}</p>` : ''}
      ${isAI(a) ? `<p class="ph-mut ph-ainote"><span class="ph-tag">${esc(t('AI-assisted'))}</span> ${esc(a.ai.by === 'developer' ? tf('Its developer says AI tools helped with its {uses}.', { uses: aiUses(a) }) : tf('Its project on GitHub shows AI tools helped with its {uses}.', { uses: aiUses(a) }))}</p>` : ''}</div>`;
  }
  const idx = () => Math.max(0, pref.tabs().indexOf(P.tab));
  const viewEl = (k) => document.getElementById('ph-v-' + k);
  // A page is patched in place once it has been drawn: covers stay and its place is kept.
  function paint(node, html) {
    if (!node) return;
    if (node.childNodes.length) morph(node, html); else node.innerHTML = html;
  }
  // Tabs off screen aren't drawn again on every change: they're marked, and drawn when they're about to
  // show (the tab's neighbours too, so a swipe never uncovers an old page), or when the phone is idle.
  const stale = new Set();
  let idleDraw = 0;
  const whenIdle = window.requestIdleCallback ? (f) => requestIdleCallback(f, { timeout: 900 }) : (f) => setTimeout(f, 200);
  function drawLater() {
    if (idleDraw || !stale.size) return;
    idleDraw = whenIdle(() => {
      idleDraw = 0;
      if (!P.on || bp) return;
      const near = [P.tab, ...neighbours()].find((k) => stale.has(k)) || [...stale][0];
      if (near) drawView(near);
      drawLater();
    });
  }
  const neighbours = () => { const t = pref.tabs(), i = t.indexOf(P.tab); return [t[i - 1], t[i + 1]].filter(Boolean); };
  function fresh(k) { if (stale.has(k)) drawView(k); }
  function drawView(k) {
    const v = viewEl(k);
    if (!v) return;
    stale.delete(k);
    paint(v, (VIEW[k] || viewHome)());
    // A tall screen shows the end of All games without a scroll: the next rows come in right away.
    if (k === 'catalog') requestAnimationFrame(() => moreGames(v));
  }
  function buildPager() {
    const tabs = pref.tabs();
    el.pager.innerHTML = tabs.map((k) => `<section class="ph-view" id="ph-v-${k}"></section>`).join('');
    if (!tabs.includes(P.tab)) P.tab = 'home';
    for (const k of tabs) stale.add(k);
    drawView(P.tab);
    drawLater();
    setPager(false);
  }
  function placeInd(at) {
    const b = $('button', el.nav), ind = $('.ph-ind', el.nav);
    if (!b || !ind) return;
    ind.style.transform = P.land ? `translateY(${at * b.offsetHeight}px)` : `translateX(${at * b.offsetWidth}px)`;
  }
  function setPager(anim) {
    // Tabs further than one swipe away skip being laid out and painted until they come near.
    const near = new Set([P.tab, ...neighbours()]);
    for (const v of el.pager.children) v.classList.toggle('far', !near.has(v.id.slice(5)));
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
    fresh(k);
    setPager(anim);
    drawLater();
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
    paint(el.mini, `<span class="pic">${img(coverUrl(a, 60))}<span class="ph-ringp">${ring(id, 40)}</span></span><span class="ph-col ph-grow"><b class="ph-gname">${esc(a.name)}</b><small data-pt="${esc(id)}">${esc(jobText(id))}</small></span>`);
    el.mini.classList.add('on');
  }
  // Everything on screen, brought up to date in place.
  function refresh() {
    if (!P.on) return;
    // Big Picture mode covers the phone layout: only it is drawn again, the rest once it closes.
    if (bp) return bpDraw();
    // Only what's on screen now: the tab showing (none while a game's page covers the tabs).
    for (const k of pref.tabs()) stale.add(k);
    if (!P.stack.length) drawView(P.tab);
    drawLater();
    const s = P.subs[P.subs.length - 1];
    if (s) { const y = el.sp.scrollTop; paint(el.sp, subHTML(s)); el.sp.scrollTop = y; consFlow(); }
    for (const box of root.querySelectorAll('[data-cta]')) { const a = appOf(box.dataset.cta); if (a) paint(box, ctaHTML(a)); }
    for (const box of root.querySelectorAll('[data-rev]')) { const a = appOf(box.dataset.rev); if (a) paint(box, revSection(a)); }
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
  let edges = { top: 0, right: 0, bottom: 0, left: 0 };
  // The design is drawn 433 wide on a phone held upright. A narrower phone scales it down to fit, a
  // tablet keeps it at full size and shows more of everything; S and L are a step either side.
  function fit() {
    // The design is drawn for a phone about 400 px across. A tablet (or a big phone held sideways in
    // a window) is drawn bigger in proportion, as if it were a large phone, so words, buttons and
    // covers keep their size against the screen instead of turning into a sea of small things.
    const short = Math.min(innerWidth, innerHeight);
    const base = short <= 433 ? short / 433 : short < 560 ? 1 : Math.min(1.7, short / 560);
    P.z = base * SIZES[bp ? pref.bpSize() : pref.size()];
    P.land = innerWidth > innerHeight;
    const px = (v) => (v / P.z).toFixed(2) + 'px';
    // Big Picture mode hides the system's bars, and Android keeps the window clear of the camera
    // hole itself (MainActivity.kt): there's nothing for the page to keep clear of.
    const insets = bp ? { top: 0, right: 0, bottom: 0, left: 0 } : edges;
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
    try { edges = Object.assign({ top: 0, right: 0, bottom: 0, left: 0 }, await invoke('android_insets')); } catch (e) { /* the last known sizes stay */ }
    fit();
  }
  window.phoneInsets = () => { if (P.on) syncInsets(); };
  addEventListener('resize', () => { if (P.on) { fit(); syncInsets(); } });
  function applyLook() {
    const theme = pref.theme();
    root.classList.toggle('ph-lite', lite());
    root.classList.toggle('ph-still', still());
    root.classList.toggle('ph-hc', pref.contrast());
    root.classList.toggle('ph-bold', pref.bold());
    root.classList.toggle('ph-solid', pref.solid());
    // Covers Only: the covers without their names under them (Settings, as on the desktop).
    root.classList.toggle('ph-bare', !!ui().hide_names);
    if (root.dataset.theme === theme) return;
    root.dataset.theme = theme;
    if (S.os === 'android') invoke('android_edges', { phone: true, light: theme === 'light' }).catch(() => {});
  }

  // ---------- starting up ----------
  // The logo over a slow wall of covers, while the list comes in. Shown once, when the app opens.
  function boot() {
    const b = el.boot;
    b.hidden = false;
    b.innerHTML = `<div class="ph-wall"></div><div class="veil"></div><div class="glow"></div><div class="mid">${mark(112)}<p class="ph-disp w">RecompLauncher</p></div><div class="ph-bootbar"><i></i></div><p class="ph-bootsay">${esc(t('Loading…'))}</p>`;
    const t0 = performance.now();
    // The covers of the last start, kept on the phone: they come from the web engine's cache, so the
    // wall is there from the first frame instead of after the list and its pictures have come in.
    let last = [];
    try { last = JSON.parse(localStorage.getItem('ph-wall') || '[]'); } catch (e) { last = []; }
    const wall = (urls) => {
      if (urls.length < 6 || $('.c', b)) return;
      let cols = '';
      for (let c = 0; c < 7; c++) {
        const set = [];
        for (let i = 0; i < 5; i++) set.push(urls[(c * 3 + i * 4) % urls.length]);
        const pics = set.concat(set).map((u) => `<img src="${esc(u)}" alt="" draggable="false">`).join('');
        cols += `<div class="c" style="animation-duration:${22 + (c % 3) * 5}s">${pics}</div>`;
      }
      const w = $('.ph-wall', b);
      w.innerHTML = cols;
      requestAnimationFrame(() => w.classList.add('on'));
    };
    const fresh = () => pool().filter((a) => a.cover).sort(byDl).slice(0, 20).map((a) => coverUrl(a, 60));
    if (Array.isArray(last) && !lite()) wall(last.filter((u) => typeof u === 'string'));
    // Gone once the games are in and the logo has had its moment; never kept waiting long offline.
    const check = () => {
      const now = fresh();
      if (now.length >= 6) { if (!lite()) wall(now); try { localStorage.setItem('ph-wall', JSON.stringify(now)); } catch (e) { /* kept next time */ } }
      const age = performance.now() - t0, ready = !!S.cat && (pool().length > 0 || !!S.listGate || S.offline);
      if ((ready && age > 1900) || age > 7000) return endBoot();
      setTimeout(check, 150);
    };
    check();
  }
  function endBoot() {
    const b = el.boot, v = viewEl(P.tab);
    P.booted = true;
    // The first time on this phone (or Run first-time setup again): a few steps before Home.
    if (S.os === 'android' && !S.settings.setup_done && !bp) setTimeout(() => ftsOpen(), 450);
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
      case 'search-cons': closeSearch(); return openSub({ t: 'cons' });
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
      case 'dev-save': {
        const v = ((root.querySelector('#ph-devname') || {}).value || '').trim();
        await saveUi({ device_name: v }, true);
        buzz('ok');
        return toast(v ? t('Saved. New saves from this phone carry that name.') : t('Back to the phone\'s own name.'));
      }
      case 'discord': return invoke('open_url', { url: discordUrl() }).catch(fail);
      case 'site': return invoke('open_url', { url: SITE }).catch(fail);
      case 'lang': return langSheet();
      case 'bpsize-roll': { const k = SIZE_KEYS[(SIZE_KEYS.indexOf(pref.bpSize()) + 1) % SIZE_KEYS.length]; buzz(); await save('phone_bp_size', k); fit(); return bp ? bpDraw() : null; }
      case 'bpstyle-roll': return bpStyleSwap();
      case 'fts-next': buzz(); P.fts = Math.min(FTS.length - 1, P.fts + 1); return ftsDraw();
      case 'fts-back': buzz(); P.fts = Math.max(0, P.fts - 1); return ftsDraw();
      case 'fts-done': return ftsDone();
      case 'self-upd': return selfUpd();
      case 'find-recomps': return findRecomps();
      case 'my-revs': return myRevsSheet();
      case 'report': return reportSheet(id);
      case 'rep-k': case 'rep-g':
        if (!P.rep) return;
        if (b.dataset.act === 'rep-k') P.rep.kind = b.dataset.k; else P.rep.game = b.dataset.g;
        b.parentNode.querySelectorAll('button').forEach((x) => x.classList.toggle('on', x === b));
        return buzz();
      case 'rep-send': return reportSend();
      case 'reset-set': return resetAsk(false);
      case 'reset-setup': return resetAsk(true);
      case 'reset-go': return resetGo(b.dataset.k === 'setup');
      case 'perm-ask': buzz(); return invoke('android_install_allowed', { ask: true }).catch(fail);
      case 'rel-t': { const box = b.closest('.ph-relx'); if (box) { box.classList.toggle('open'); b.setAttribute('aria-expanded', String(box.classList.contains('open'))); } return buzz(); }
      case 'rel-pick': return relPick(id, b.dataset.tag);
      case 'down-go': return downGo(id, b.dataset.tag);
      case 'url': return invoke('open_url', { url: b.dataset.url }).catch(fail);
      case 'notes': return notesSheet(id);
      case 'vers': return versSheet(id);
      case 'rels': return relsSheet(id);
      case 'rev-say': buzz(); return window.askRun && window.askRun(id, (S.library[id] || {}).played_secs || 0, true);
      case 'rev-all': return revAllSheet(id);
      case 'rev-v': { const y = el.sheet.scrollTop; revAllSheet(id, b.dataset.v); el.sheet.scrollTop = y; return; }
      case 'rv-v': buzz(); return rvPickIt(b.dataset.v);
      case 'rv-send': return rvSend();
      case 'rv-del': return rvDelete(b);
      case 'rv-cancel': P.rv = null; return closeSheet();
      case 'refresh-cat': {
        buzz();
        // Once a minute at most: the list is the same a moment later.
        if (Date.now() - (S.refreshedAt || 0) < 60e3) return toast(t('The Catalog is up to date.'));
        S.refreshedAt = Date.now();
        toast(t('Getting the newest list…'));
        await loadCatalog(true);
        if (typeof androidSync === 'function') await androidSync();
        S.rels = {};
        toast(offNow() ? t('Can\'t reach recomplauncher.com right now. Try again in a bit.') : t('The Catalog is up to date.'));
        return refresh();
      }
      case 'check-upd': {
        buzz();
        b.disabled = true;
        const fresh = Date.now() - (S.updCheckedAt || 0) >= 60e3;
        if (fresh) S.updCheckedAt = Date.now();
        if (fresh) { await loadCatalog(true); if (typeof androidSync === 'function') await androidSync(); }
        const ok = await checkUpdates(fresh);
        b.disabled = false;
        const n = updatesList().length;
        toast(n ? tn(n, '1 update ready', '{n} updates ready') : ok && offNow() ? t('Can\'t reach recomplauncher.com right now. Try again in a bit.') : t('Everything is up to date.'));
        return refresh();
      }
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
    if (key === 'padstick') return saveSettings({ pad_stick: on }, true);
    if (key === 'bpstart') return saveSettings({ start_in_tv: on }, true);
    if (key === 'lite' || key === 'still' || key === 'contrast') {
      await save({ lite: 'phone_lite', still: 'phone_still', contrast: 'phone_contrast' }[key], on);
      applyLook();
      if (bp) bpDraw();
      return drawView('settings');
    }
    if (key === 'bold' || key === 'solid') { await save(key === 'bold' ? 'phone_bold' : 'phone_solid', on); applyLook(); if (bp) bpDraw(); return; }
    // The desktop's own settings, so a PC and this phone agree on them.
    if (key === 'bare') { await saveUi({ hide_names: on }, true); applyLook(); return; }
    if (key === 'cvl' || key === 'gnames' || key === 'gmedia') { await saveUi({ [{ cvl: 'cover_labels', gnames: 'game_names', gmedia: 'game_media' }[key]]: on }, true); return refresh(); }
    if (key === 'usage' || key === 'stats') return saveSettings({ [key === 'usage' ? 'share_usage' : 'share_stats']: on, data_asked: true }, true);
    if (key === 'hideai') { await saveSettings({ hide_ai: on }, true); return refresh(); }
    if (key === 'revask') return window.setReviewAsk && window.setReviewAsk(on);
    if (key === 'revnever') return window.setReviewAsk && window.setReviewAsk(!on);
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
    if (key === 'size') { await save('phone_size', SIZE_KEYS[i]); fit(); }
    if (key === 'bpsize') { await save('phone_bp_size', SIZE_KEYS[i]); fit(); }
    if (key === 'bpstyle') { await save('bp_style', i ? 'full' : 'phone'); drawView('settings'); }
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
    if (d.bp) return bpAct(b);
    // A row of Big Picture's game menu closes it as it does its thing.
    if (d.bpm && bp) { Object.assign(bp, { menu: '', f: bp.mback || '' }); bpDraw(); }
    if (d.act) return act(b);
    if (d.sw) return flip(b);
    if (d.sgi !== undefined) return pick(b);
    if (d.menu) return bp ? bpMenu(d.menu) : menuSheet(d.menu);
    if (d.readme) return readmeSheet(d.readme);
    if (d.lang) return setLanguage(d.lang).then(() => { buildPager(); drawNav(); });
    if (d.sub) {
      if (P.stack.length) closeGame(true);
      return openSub({ t: d.sub });
    }
    if (d.con) { closeSearch(); return openSub({ t: 'con', k: d.con }); }
    if (d.open) return bp ? bpGame(d.open) : openGame(d.open, b);
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
    if (e.target.classList.contains('ph-rvta')) rvCheck();
    // Big Picture's Catalog narrows as the player types.
    if (bp && e.target.closest('.ph-bpq')) { bp.q = e.target.value; bp.n = BP_STEP; bpDraw(); }
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
      if (n.id === 'ph-v-catalog') return moreGames(n);
      if (n.classList.contains('ph-bpscroll')) { if (bp) bpScrolled(n); return bpMore(n); }
      if (still()) return;
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

  // The next rows of All games, once the page is near its end. A screenful stays ready below.
  function moreGames(n) {
    if (P.allN >= pool().length || n.scrollTop + n.clientHeight < n.scrollHeight - n.clientHeight * 1.5) return;
    P.allN += ALL_STEP;
    drawView('catalog');
  }

  // ---------- swipes: between tabs, back from the edge, a sheet pulled down, a long press ----------
  let G = null;
  // A game's big picture starts loading as the finger lands on its cover, so its page opens with it.
  const warmed = new Set();
  function warm(id) {
    const a = appOf(id), u = a && pageArt(a);
    if (!u || warmed.has(u) || SEEN.has(u)) return;
    warmed.add(u);
    const i = new Image();
    i.decoding = 'async';
    i.src = u;
  }
  root.addEventListener('pointerdown', (e) => {
    const o = e.target.closest && e.target.closest('[data-open]');
    if (o) warm(o.dataset.open);
    dragged = false; pressed = false;
    clearTimeout(hold);
    // Big Picture mode: fingers tap and scroll; the controller's highlight steps aside.
    if (bp) { G = null; if (bp.pad) { bp.pad = false; bpDraw(); } return; }
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
        else { G.ax = 'pager'; neighbours().forEach(fresh); el.pager.classList.remove('anim'); el.nav.classList.add('drag'); }
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

  // Rubber band: a list pulled past its top or end gives a little and springs back, instead of
  // stopping dead. Touch events are only listened to (never held up), so scrolling stays the phone's own.
  const BAND = '.ph-view,.ph-gp,.ph-sp,.ph-so,.ph-bpscroll';
  let R = null;
  root.addEventListener('touchstart', (e) => {
    const n = e.touches.length === 1 && e.target.closest && e.target.closest(BAND);
    R = n && !still() && !e.target.closest('.ph-sheet,input,textarea') ? { n, y: e.touches[0].clientY, x: e.touches[0].clientX, edge: null, d: 0 } : null;
  }, { passive: true, capture: true });
  root.addEventListener('touchmove', (e) => {
    if (!R || dragged) return;
    const { n } = R, y = e.touches[0].clientY;
    if (R.edge === null) {
      if (Math.abs(e.touches[0].clientX - R.x) > Math.abs(y - R.y) + 4) { R = null; return; }
      const top = n.scrollTop <= 0, end = n.scrollTop + n.clientHeight >= n.scrollHeight - 1;
      if (top && y > R.y + 2) R.edge = 1;
      else if (end && y < R.y - 2) R.edge = -1;
      else { if (!top && !end) R.y = y; return; }
      R.y = y;
    }
    const pull = Math.max(0, (y - R.y) * R.edge) / P.z, h = n.clientHeight || 600;
    // Harder to pull the further it goes, never more than a tenth of the screen.
    R.d = R.edge * (1 - 1 / (pull * 0.5 / h + 1)) * h * 0.22;
    n.style.translate = `0 ${R.d.toFixed(1)}px`;
  }, { passive: true, capture: true });
  const unband = () => {
    if (!R) return;
    const { n, d } = R;
    R = null;
    if (!d) return;
    n.style.translate = '';
    if (n.animate) n.animate([{ translate: `0 ${d.toFixed(1)}px` }, { translate: '0 0' }], { duration: 520, easing: 'cubic-bezier(.3,1.35,.5,1)' });
  };
  root.addEventListener('touchend', unband, { passive: true, capture: true });
  root.addEventListener('touchcancel', unband, { passive: true, capture: true });

  // Android's Back: the nearest thing closes, then Home, then out of the app (false).
  function back() {
    if (P.sheet) { closeSheet(); return true; }
    if (P.search) { closeSearch(); return true; }
    if (P.stack.length) { backGame(); return true; }
    if (P.subs.length) { closeSub(); return true; }
    if (P.tab !== 'home') { go('home'); return true; }
    return false;
  }

  // ---------- Big Picture mode ----------
  // Full screen and sideways on a phone, for a controller and for fingers alike (a tablet, a handheld
  // and a TV have the big-screen one, tv.js). The same sections as that one: Home (the focused game's
  // art over a shelf of covers, and rows under it), Library, Catalog, Downloads, Achievements and
  // Settings, a game's full page (reviews, what it is, its README, notes and updates) and its menu.
  // Drawn over the phone layout, which waits underneath as it was.
  const BP_TABS = [['home', N_('Home')], ['library', N_('Library')], ['catalog', N_('Catalog')], ['downloads', N_('Downloads')], ['ach', N_('Achievements')], ['settings', N_('Settings')]];
  const BP_ICON = { home: 'home', library: 'library', catalog: 'catalog', downloads: 'downloads', ach: 'trophy', settings: 'settings' };
  const BP_STEP = 36;
  const GLYPHS = [['', N_('Automatic')], ['xbox', 'Xbox'], ['ps', 'PlayStation'], ['nintendo', 'Nintendo'], ['steam', 'Steam']];
  const bpTabKey = () => BP_TABS[bp.tab][0];
  const bpTabOf = (k) => BP_TABS.findIndex(([x]) => x === k);
  const dlIds = () => uniq([...Object.keys(S.jobs), ...Object.keys(S.paused || {})]);
  // Home's shelf: the player's own games first, then the week's top 10.
  function bpShelf() {
    const mine = mineList(), have = new Set(mine.map((a) => a.id));
    return mine.concat(ranked().filter((a) => !have.has(a.id))).slice(0, 24);
  }
  function bpPool() {
    let list = [...pool()].sort((x, y) => x.name.localeCompare(y.name));
    if (bp.con) list = list.filter((a) => a.origin === bp.con);
    const q = (bp.q || '').trim().toLowerCase();
    if (q) list = list.filter((a) => [a.name, a.game, conName(a), devName(a)].join(' ').toLowerCase().includes(q));
    return list;
  }
  const bpGrid = () => (bpTabKey() === 'library' ? mineList() : bpPool());
  // The controller's own button pictures (Xbox letters, PlayStation shapes), as in tv.js.
  const glyph = (n) => {
    if (!window.padArt) return '';
    const p = padArt(ui().tv_glyphs || (typeof padKind === 'function' ? padKind() : S.padKind) || 'xbox', n);
    return `<i class="glyph" style="--m:url(${p.url})" role="img" aria-label="${esc(t(p.name))}"></i>`;
  };
  const bpCover = (a, key, sub) => `<button type="button" class="ph-bpc ph-press" data-bp="game" data-id="${esc(a.id)}" data-key="${esc(key)}" aria-label="${esc(a.name)}">${coverBox(a, isBusy(a) ? 'dim' : '', '', 180)}${isBusy(a) ? `<span class="ph-ringp">${ring(a.id, 40)}</span>` : ''}${sub == null ? '' : `<span class="ph-gname" dir="auto">${esc(a.name)}</span><span class="ph-gsub">${sub}</span>`}</button>`;
  function bpInfo(a, full) {
    const id = esc(a.id), lib = S.library[a.id];
    const tags = (a.origin ? `<span class="ph-tag lt">${esc(conName(a))}</span>` : '') + (GENRES[a.genre] ? `<span class="ph-tag dark">${esc(t(GENRES[a.genre]))}</span>` : '') + (stateOf(a) === 'update' ? `<span class="ph-tag acc">${esc(t('Update ready'))}</span>` : '');
    const title = a.logo ? img(media(a.logo), 'logo', 'data-logo') + `<h1 class="ph-disp" dir="auto" hidden>${esc(a.name)}</h1>` : `<h1 class="ph-disp" dir="auto">${esc(a.name)}</h1>`;
    const by = devName(a) ? tf('by {who}', { who: devName(a) }) : '';
    // On its page: what it's based on and who made it, with the play time in its own boxes below.
    const line = full ? [a.logo ? a.name : '', a.game ? tf('Based on {game}', { game: a.game }) : '', by].filter(Boolean).join(' · ')
      : [a.logo ? a.name : '', lib ? playedFor(lib) : by, lib ? '' : sizeOf(a)].filter(Boolean).join(' · ');
    // The score players gave it, in a word: on Home and on its page.
    const c = runTally((S.runCounts || {})[a.id]);
    const score = c.n ? `<span class="ph-bpscore ${rvTone(c.pct('runs'))}">${esc(tf('{pct}% say it plays great', { pct: c.pct('runs') }))} · ${esc(tn(c.n, '1 review', '{n} reviews'))}</span>` : '';
    return `<div class="ph-bpinfo${full ? ' full' : ''}" data-key="i:${id}"><div class="tags">${tags}</div>${title}${line ? `<p class="line" dir="auto">${esc(line)}</p>` : ''}${score}${full ? playBox(a) : ''}
      <div class="ph-row ph-bpacts"><div class="ph-cta" data-cta="${id}">${ctaHTML(a)}</div>${full ? '' : `<button type="button" class="ph-btn sec ph-press" data-bp="game" data-id="${id}">${esc(t('Details'))}</button>`}</div>
      ${a.soon && full ? `<p class="ph-bpab" dir="auto">${esc(soonNoteText(a))}</p>` : ''}</div>`;
  }
  function bpSide(a) {
    const facts = [[t('Console'), conName(a)], [t('Developer'), devName(a)], [t('version'), a.version ? String(verText(a.version)) : ''], [t('size'), sizeOf(a)], [t('players'), Number(a.players) > 0 ? fmtN(Number(a.players)) : ''], [t('downloads'), a.downloads ? fmtN(a.downloads) : ''], [t('Latest release'), a.released ? date(a.released) : ''], [t('Genre'), GENRES[a.genre] ? t(GENRES[a.genre]) : '']].filter(([, v]) => v);
    if (!facts.length) return '';
    return `<div class="ph-bpside" data-key="side:${esc(a.id)}">${facts.map(([k, v]) => `<div><span>${esc(k)}</span><b dir="auto">${esc(v)}</b></div>`).join('')}</div>`;
  }
  // Home: the shelf and the focused game, and the rows a press of down away.
  function bpRows() {
    const all = pool();
    const played = mineList().filter((a) => S.library[a.id] && S.library[a.id].last_played);
    const saved = savedItems().map((x) => x.a).filter((a) => a && forOS(a));
    const rows = [
      ['cont', t('Continue playing'), played, (a) => esc(playedFor(S.library[a.id]))],
      ['trend', t('Trending this week'), ranked(), null],
      ['saved', t('Saved for later'), saved, null],
      ['upd', t('Recently updated'), recentlyUpdated(all), (a) => esc(a.version ? 'v' + String(verText(a.version)).replace(/^v/i, '') : '')],
      ['new', t('Just added'), [...all].sort(byNew), null],
      ['pop', t('Popular on Android'), [...all].sort(byDl), null],
    ];
    return rows.filter((r) => r[2].length).map(([k, title, list, sub]) => `<section class="ph-bpsec" data-key="row:${k}"><h3>${esc(title)}</h3><div class="ph-bpline" data-key="line:${k}">${list.slice(0, 14).map((a, i) => bpCover(a, `${k}:${a.id}`, sub ? sub(a) : k === 'trend' ? `#${i + 1}` : listLine(a))).join('')}</div></section>`).join('');
  }
  function bpHome() {
    const sh = bpShelf();
    if (!sh.length) return `<div class="ph-bpnone" data-key="none">${S.listGate ? gate() : `<span class="ph-spin"></span><span>${esc(S.offline ? t("Can't reach recomplauncher.com right now.") : t('Loading…'))}</span>`}</div>`;
    if (!sh.some((a) => a.id === bp.focus)) bp.focus = sh[0].id;
    const shelf = sh.map((a) => `<button type="button" class="ph-press${a.id === bp.focus ? ' f' : ''}" data-bp="focus" data-id="${esc(a.id)}" data-key="s:${esc(a.id)}" aria-label="${esc(a.name)}">${coverBox(a, '', '', 180)}</button>`).join('');
    return `<div class="ph-bpscroll home" data-key="home"><section class="ph-bphero" data-key="hero">${bpInfo(appOf(bp.focus))}<div class="ph-bpshelf" data-key="shelf">${shelf}</div></section><div class="ph-bprows" data-key="rows">${bpRows()}</div></div>`;
  }
  // Library and Catalog: covers in a grid, a few rows at a time. The Catalog narrows by console or
  // by a search.
  function bpGridHTML() {
    const lib = bpTabKey() === 'library';
    const list = bpGrid(), shown = list.slice(0, bp.n);
    let head = '';
    if (lib) {
      const n = Object.keys(S.library).length;
      head = `<div class="ph-bphead" data-key="lh"><h2>${esc(t('Library'))}</h2><span class="ph-mut">${esc(tn(n, '1 game', '{n} games'))}</span></div>`;
    } else {
      const cons = consoles();
      const chip = (k, name) => `<button type="button" class="ph-chip ph-press${(bp.con || '') === k ? ' on' : ''}" data-bp="con" data-k="${esc(k)}" data-key="con:${esc(k)}">${k ? conDot(k) : ''}${esc(name)}</button>`;
      head = `<div class="ph-bphead" data-key="ch"><h2>${esc(t('Catalog'))}</h2><span class="ph-mut">${esc(tn(list.length, '1 game', '{n} games'))}</span><span class="ph-grow"></span><label class="ph-bpq">${ic('search', 's')}<input type="search" enterkeyhint="search" data-key="q" placeholder="${esc(t('Search games, consoles, developers'))}" value="${esc(bp.q || '')}"></label></div>
        <div class="ph-bpchips2" data-key="cons">${chip('', t('All'))}${cons.map((c) => chip(c.k, c.name)).join('')}</div>`;
    }
    const none = lib ? t('Your games show up here once you install one.') : pool().length ? t('Nothing matches that.') : t('Loading…');
    const saved = lib ? savedItems().map((x) => x.a).filter((a) => a && forOS(a) && !a.soon) : [];
    const later = saved.length ? `<div class="ph-bphead" data-key="sv"><h2>${esc(t('Saved for later'))}</h2></div><div class="ph-bpgrid" data-key="svg">${saved.map((a) => bpCover(a, 'sv:' + a.id, listLine(a))).join('')}</div>` : '';
    return `<div class="ph-bpscroll" data-key="grid:${bp.tab}">${head}${shown.length ? `<div class="ph-bpgrid" data-key="gg">${shown.map((a) => bpCover(a, 'g:' + a.id, lib ? libLine(a) : listLine(a))).join('')}</div>${shown.length < list.length ? '<div class="ph-gridmore" data-key="more"><span class="ph-spin"></span></div>' : ''}` : `<div class="ph-bpnone">${esc(none)}</div>`}${later}</div>`;
  }
  // Downloads: what's coming down now, then the updates ready for the player's games.
  function bpDownloads() {
    const dls = dlIds().map(appOf).filter(Boolean), ups = updatesList();
    const up = (a) => lrow(a, { sub: esc([a.version ? tf('Version {v}', { v: verText(a.version) }) : '', sizeOf(a)].filter(Boolean).join(' · ')), right: `<button type="button" class="ph-btn pri sm ph-press" data-act="update" data-id="${esc(a.id)}">${esc(t('Update'))}</button>` });
    return `<div class="ph-bpscroll" data-key="dl"><div class="ph-bplist">
      <div class="ph-bphead" data-key="dh"><h2>${esc(t('Downloads'))}</h2><span class="ph-mut">${esc(t('Downloads keep going while you browse.'))}</span></div>
      ${dls.length ? dls.map(dlRow).join('') : `<div class="ph-bpempty" data-f data-key="dnone">${ic('downloads')}<span>${esc(t('Nothing is downloading.'))}</span></div>`}
      <div class="ph-bphead" data-key="uh"><h2>${esc(t('Updates'))}</h2>${ups.length > 1 ? `<button type="button" class="ph-btn sec sm ph-press" data-act="update-all">${esc(t('Update all'))}</button>` : ''}</div>
      ${ups.length ? ups.map(up).join('') : `<button type="button" class="ph-bpempty ph-press" data-act="check-upd" data-key="unone">${ic('check')}<span>${esc(t('Everything is up to date.'))}<small>${esc(t('Check for Game Updates'))}</small></span></button>`}
    </div></div>`;
  }
  // Achievements: the player's profile, Beaten and Mastered games, recent unlocks and the
  // leaderboard for testers; what it is and how to join for everyone else.
  function bpAch() {
    const AT = window.achTV, pg = AT ? AT.page() : { tester: false };
    const signed = !!(S.account && S.account.signedIn);
    const star = ic('star');
    if (!pg.tester) {
      const st = pg.status || {}, app = st.application, pending = signed && app && app.status === 'pending';
      const feats = [[t('Unlocks While You Play'), t('Achievements pop up in the corner the moment you earn them, right over your recomp.')], [t('Your Profile and Rating'), t('Every achievement adds to your points and rank, on every PC you play on.')], [t('Leaderboards'), t('See how you stack up against other players, overall and game by game. Only Ranked unlocks count.')]];
      const line = !signed ? t('Sign in, then apply to be a tester on the Achievements page in the desktop view.') : pending ? t('You\'re on the list. We approve testers in waves, and this page unlocks on its own once you\'re in.') : t('Apply to be a tester on the Achievements page in the desktop view.');
      return `<div class="ph-bpscroll" data-key="ach"><div class="ph-bplist">
        <div class="ph-bphead" data-key="ah"><h2>${esc(t('Achievements'))}</h2><span class="ph-tag acc">${esc(pending ? t('On the List') : t('Alpha'))}</span></div>
        <p class="ph-mut ph-bplead">${esc(t('Earn achievements in your recomps, track them on your profile and climb the leaderboards.'))}</p>
        <div class="ph-bpfeats">${feats.map(([h, d], i) => `<div class="ph-panel" data-f data-key="af:${i}">${star}<b>${esc(h)}</b><p>${esc(d)}</p></div>`).join('')}</div>
        <p class="ph-bpnote" data-f data-key="aline">${esc(line)}</p>
        ${signed ? '' : `<button type="button" class="ph-btn pri ph-press" data-act="signin" data-key="ain">${esc(t('Sign in'))}</button>`}</div></div>`;
    }
    const p = pg.profile || {}, u = (S.account && S.account.user) || {};
    const box = (v, k) => `<div><b>${esc(v)}</b><span>${esc(k)}</span></div>`;
    const ms = (p.mastered || []).map((m) => Object.assign({ kind: 'm' }, m));
    const done = ms.concat((p.beaten || []).filter((b) => !ms.some((m) => m.id === b.id)).map((b) => Object.assign({ kind: 'b' }, b)));
    const recent = (p.recent || []).slice(0, 6), rows = (pg.board || []).slice(0, 10);
    const gm = (m) => { const a = appOf(m.id) || { id: m.id, name: m.name || m.id, cover: m.cover }; return bpCover(a, 'ad:' + m.id, esc(m.kind === 'm' ? t('Mastered') : t('Beaten'))); };
    return `<div class="ph-bpscroll" data-key="ach"><div class="ph-bplist">
      <div class="ph-bphead" data-key="ah"><h2>${esc(u.username || t('Tester'))}</h2><span class="ph-tag acc">${esc(t('Tester'))}</span></div>
      <div class="ph-panel ph-bpstats" data-f data-key="aprof">${box(fmtN(p.points || 0), t('Points'))}${box(fmtN(p.score || 0), t('Score'))}${box(fmtN(p.beaten_count || (p.beaten || []).length || 0), t('Beaten'))}${box(fmtN(p.mastered_count || 0), t('Mastered'))}</div>
      <div class="ph-bphead" data-key="adh"><h2>${esc(t('Beaten and Mastered'))}</h2></div>
      ${done.length ? `<div class="ph-bpline" data-key="adl">${done.map(gm).join('')}</div>` : `<p class="ph-bpnote" data-f data-key="adn">${esc(t('Beat a game and it lands here; unlock every achievement and it\'s Mastered.'))}</p>`}
      <div class="ph-bphead" data-key="arh"><h2>${esc(t('Recent Unlocks'))}</h2></div>
      ${recent.length ? recent.map((r, i) => `<button type="button" class="ph-bprow ph-press" data-bp="game" data-id="${esc(r.game)}" data-key="ar:${i}">${star}<span class="ph-col ph-grow"><span>${esc(r.title || tf('Achievement {n}', { n: r.id }))}</span><small>${esc([r.name || (appOf(r.game) || {}).name || '', r.rarity != null && AT ? AT.rarityText(r.rarity) : ''].filter(Boolean).join(' · '))}</small></span></button>`).join('') : `<p class="ph-bpnote" data-f data-key="arn">${esc(t('Your unlocks show here the moment you earn them.'))}</p>`}
      ${rows.length ? `<div class="ph-bphead" data-key="alh"><h2>${esc(t('Leaderboard'))}</h2></div><div class="ph-panel ph-bplb" data-f data-key="alb">${rows.map((r) => `<div class="${r.username === u.username ? 'you' : ''}"><span>${esc(String(r.rank || ''))}</span><b>${esc(r.username || '')}</b><em>${esc(fmtN(r.points || 0))}</em></div>`).join('')}</div>` : ''}
    </div></div>`;
  }
  // A game's achievements, on its page: how far the player is and the list (testers only).
  function bpAchGame(a) {
    const AT = window.achTV, d = AT && AT.game(a.id);
    const list = d && d.allowed !== false && d.list && Array.isArray(d.list.achievements) ? d.list.achievements.filter((x) => !AT.isBonus(x)) : [];
    if (!list.length) return '';
    const have = new Set((d.unlocked || []).map(Number)), got = list.filter((x) => have.has(Number(x.id))).length;
    return `<div class="ph-panel ph-bpachg" data-f data-key="ag:${esc(a.id)}"><h2>${esc(t('Achievements'))}</h2><p class="ph-mut">${esc(tf('{n} of {all} unlocked', { n: got, all: list.length }))}</p><div class="ph-revbar"><i style="flex:${got}"></i><i class="v-none" style="flex:${list.length - got}"></i></div>
      ${list.slice(0, 6).map((x) => `<div class="row${have.has(Number(x.id)) ? ' got' : ''}">${ic('star', 's')}<span class="ph-col"><b>${esc(x.title || '')}</b><small>${esc(x.description || '')}</small></span></div>`).join('')}</div>`;
  }
  // A game's own page: everything the phone layout's page has, laid out for a sideways screen.
  function bpGameHTML(a) {
    const id = esc(a.id), more = moreLike(a, 12), tags = tagsOf(a);
    const g = verGroup(a);
    const vers = g ? pageRow('layers', tn(g.list.length, '1 version', '{n} versions'), g.why || g.name, `data-act="vers" data-id="${id}" data-key="vers:${id}"`) : '';
    const right = `${vers}${pageRows(a)}${bpAchGame(a)}${tags.length ? `<div class="ph-chips ph-bpchips" data-key="tags">${tags.map((x) => `<span class="ph-chip">${esc(x)}</span>`).join('')}</div>` : ''}`;
    return `<div class="ph-bpscroll game" data-key="gp:${id}">
      <section class="ph-bpgtop" data-key="gtop">${bpInfo(a, true)}${bpSide(a)}</section>
      <div class="ph-bpgbody" data-key="gbody"><div class="ph-col" data-key="gl">${gameAbout(a, true)}<div class="ph-revbox" data-rev="${id}">${revSection(a)}</div></div><div class="ph-col" data-key="gr">${right}</div></div>
      ${more.length ? `<section class="ph-bpsec" data-key="row:like"><h3>${esc(t('More like this'))}</h3><div class="ph-bpline" data-key="line:like">${more.map((x) => bpCover(x, 'like:' + x.id, listLine(x))).join('')}</div></section>` : ''}
    </div>`;
  }
  function bpMenuHTML(a) {
    const st = stateOf(a), id = esc(a.id), lib = S.library[a.id];
    const row = (icon, label, attr, cls = '') => `<button type="button" class="ph-bprow ph-press ${cls}" data-bpm="1" ${attr}>${ic(icon, 's')}<span>${esc(label)}</span></button>`;
    const rows = [];
    if (st === 'installed' || st === 'running') rows.push(row('play', t('Play'), `data-act="play" data-id="${id}"`));
    if (st === 'update') rows.push(row('downloads', t('Update'), `data-act="update" data-id="${id}"`), row('play', t('Play'), `data-act="play" data-id="${id}"`));
    if (st === 'get') rows.push(row('downloads', t('Install'), `data-act="install" data-id="${id}"`));
    if (st === 'paused') rows.push(row('downloads', t('Resume'), `data-act="resume" data-id="${id}"`));
    if (st === 'busy' || st === 'paused') rows.push(row('x', t('Cancel'), `data-act="cancel" data-id="${id}"`));
    if (bp.game !== a.id) rows.push(`<button type="button" class="ph-bprow ph-press" data-bp="game" data-id="${id}">${ic('chev', 's')}<span>${esc(t('Details'))}</span></button>`);
    if (lib && reviewable(a)) rows.push(row('pen', myReviewOf(a.id) ? t('Edit Your Review') : t('Write a Review'), `data-act="rev-say" data-id="${id}"`));
    if (reviewable(a) && S.reports[a.id] && (S.reports[a.id].items || []).length) rows.push(row('star', t('Reviews'), `data-act="rev-all" data-id="${id}"`));
    if (a.repo && !a.local) rows.push(row('file', t('Read the README'), `data-readme="${id}"`), row('up', t('Updates'), `data-act="rels" data-id="${id}"`));
    if (!lib && !a.custom) rows.push(row('bookmark', isSaved(a.id) ? t('Remove from saved') : t('Save for later'), `data-act="save" data-id="${id}"`));
    if (a.repo && !a.local) rows.push(row('globe', t('Open the project page'), `data-act="project" data-id="${id}"`));
    if (lib && st !== 'busy' && st !== 'uninstalling') rows.push(row('trash', t('Uninstall'), `data-act="uninstall" data-id="${id}"`, 'red'));
    revLoad(a);
    return `<div class="ph-bpmenu" data-key="m:${id}"><div class="ph-bpwho">${coverBox(a)}<b dir="auto">${esc(a.name)}</b><small>${esc([a.version ? 'v' + String(verText(a.version)).replace(/^v/i, '') : '', sizeOf(a)].filter(Boolean).join(' · '))}</small></div><div class="rows">${rows.join('')}</div></div>`;
  }
  // Settings: the phone layout's, and the big-screen Big Picture mode's, that matter on a phone.
  function bpSettings() {
    const acc = S.account || {}, user = acc.signedIn && acc.user ? acc.user : null;
    const head = (k, title) => `<h3 class="ph-bph3" data-key="sh:${k}">${esc(title)}</h3>`;
    const row = (title, sub, key, on, off) => `<div class="ph-bprow set"><span class="ph-col ph-grow"><span>${esc(title)}</span>${sub ? `<small>${esc(sub)}</small>` : ''}</span>${sw(key, on, off)}</div>`;
    const go = (icon, title, sub, attr, val = '') => `<button type="button" class="ph-bprow ph-press" ${attr}>${ic(icon, 's')}<span class="ph-col ph-grow"><span>${esc(title)}</span>${sub ? `<small>${esc(sub)}</small>` : ''}</span>${val ? `<b class="val">${esc(val)}</b>` : ''}</button>`;
    const g = GLYPHS.find(([k]) => k === (ui().tv_glyphs || '')) || GLYPHS[0];
    const lang = (LANGS.find((l) => l[0] === ui().lang) || [])[1] || t('Same as your phone');
    const ups = updatesList().length;
    return `<div class="ph-bpscroll" data-key="set"><div class="ph-bpset">
      ${head('acc', t('Account'))}
      ${user ? go('user', tf('Signed in as {name}', { name: user.username }), t('Your saved games and reviews follow you.'), 'data-act="account-site" data-key="s-acc"') : go('user', t('Sign in'), t('Keep your saved games with your account, and write reviews.'), 'data-act="signin" data-key="s-acc"')}
      ${head('bp', t('Big Picture mode'))}
      ${go('tv', t('Big Picture style'), t('Phone is made for this screen. Full is the big-screen one tablets and PCs have.'), 'data-act="bpstyle-roll" data-key="s-style"', pref.bpStyle() === 'full' ? t('Full') : t('Phone'))}
      ${go('text', t('Big Picture size'), t('Bigger or smaller text, buttons and covers in Big Picture'), 'data-act="bpsize-roll" data-key="s-size"', SIZE_LABELS[SIZE_KEYS.indexOf(pref.bpSize())])}
      ${row(t('Start in Big Picture mode'), t('Opens full screen, ready for a controller. Good for a TV or a handheld.'), 'bpstart', !!S.settings.start_in_tv)}
      ${row(t('Open with a controller'), t('Switches when you press a button on one'), 'padtv', pref.padTv())}
      ${go('pad', t('Button Icons'), t('Which controller\'s buttons are shown. Automatic follows the one you use.'), 'data-bp="glyphs" data-key="s-gl"', t(g[1]))}
      ${row(t('Move with the left stick'), t('Turn it off if your stick drifts. The d-pad still works.'), 'padstick', S.settings.pad_stick !== false)}
      ${row(t('Vibrate on taps'), '', 'haptics', pref.haptics())}
      ${head('covers', t('Covers and names'))}
      ${row(t('Covers Only'), t('Hide the names under game covers.'), 'bare', !!ui().hide_names)}
      ${row(t('Console Labels on Covers'), t('A strip across the top of each cover with its console, like the band on a real game box.'), 'cvl', !!ui().cover_labels)}
      ${row(t('Show Game Names'), t('Name every recomp after its game, like Tears of the Kingdom instead of TotK Recomp. Names you gave games yourself stay.'), 'gnames', !!ui().game_names)}
      ${row(t('Show Game Media'), t('Every version of a game shows the same cover, background and logo: the recommended version\'s.'), 'gmedia', !!ui().game_media)}
      ${head('cat', t('Catalog and updates'))}
      ${row(t('Hide AI-assisted Recomps'), t('They won\'t show in the Catalog'), 'hideai', !!S.settings.hide_ai)}
      ${go('refresh', t('Refresh Catalog'), t('Get the newest list of recomps, their releases and their art now'), 'data-act="refresh-cat" data-key="s-ref"')}
      ${go('downloads', t('Check for Game Updates'), ups ? tn(ups, '1 update ready', '{n} updates ready') : t('Looks for new releases of your games now'), 'data-act="check-upd" data-key="s-upd"')}
      ${go('phone', t('Check for Launcher Updates'), tf('You have {v}', { v: S.version || '' }), 'data-act="self-upd" data-key="s-self"')}
      ${go('box', t('Bring Your Own Recomps'), t('Finds recomps already installed on this phone and adds them to your Library'), 'data-act="find-recomps" data-key="s-find"')}
      ${head('rev', t('Reviews'))}
      ${row(t('Ask After Playing'), t('After you play a game for a while, the launcher asks how it ran. Never again for a game you\'ve reviewed.'), 'revask', window.reviewAskOn ? window.reviewAskOn() : true)}
      ${go('star', t('Your Reviews'), t('Every review you wrote, with any answer from its developer'), 'data-act="my-revs" data-key="s-myrev"')}
      ${head('ease', t('Accessibility'))}
      ${row(t('Bold text'), t('Heavier letters everywhere, easier to read'), 'bold', pref.bold())}
      ${row(t('High contrast'), t('Stronger text and edges'), 'contrast', pref.contrast())}
      ${row(t('Reduce transparency'), t('Solid menus and buttons instead of see-through ones'), 'solid', pref.solid())}
      ${row(t('Reduce motion'), t('Pages change without sliding or zooming'), 'still', pref.still() || pref.lite(), pref.lite())}
      ${row(t('Optimization mode'), t('Flat and still: no blurs, glows or moving extras. Smoother on older phones.'), 'lite', pref.lite())}
      ${head('help', t('Help and privacy'))}
      ${go('bug', t('Report a Problem'), t('Something broke, an idea, or what you think so far. It goes straight to the developer.'), 'data-act="report" data-key="s-rep"')}
      ${row(t('Share usage and diagnostics'), t('Your phone\'s model, which games you play and for how long, and what went wrong when one won\'t install. No names, no files.'), 'usage', !!S.settings.share_usage)}
      ${row(t('Share anonymous usage stats'), t('When the launcher is open and which recomps are being played, under a random ID that isn\'t linked to you or your account.'), 'stats', !!S.settings.share_stats)}
      ${head('over', t('Start over'))}
      ${go('undo', t('Reset settings'), t('Every setting goes back to how it started. Your games and saves stay.'), 'data-act="reset-set" data-key="s-reset"')}
      ${go('spark', t('Run first-time setup again'), t('Nothing is deleted.'), 'data-act="reset-setup" data-key="s-setup"')}
      ${head('about', t('About'))}
      ${go('lang', t('Language'), '', 'data-act="lang" data-key="s-lang"', lang)}
      ${go('discord', 'Discord', t('Help, news and other players'), 'data-act="discord" data-key="s-dc"')}
      ${go('globe', 'recomplauncher.com', '', 'data-act="site" data-key="s-site"')}
      <div class="ph-bprow set" data-f data-key="s-ver"><span class="ph-col ph-grow"><span>${esc(tf('RecompLauncher {v}', { v: S.version || '' }))}</span><small>${esc(engine())}</small></span></div>
      <button type="button" class="ph-bprow ph-press" data-bp="exit" data-key="exitrow">${ic('phone', 's')}<span>${esc(t('Leave Big Picture mode'))}</span></button></div></div>`;
  }
  function bpHTML() {
    const pad = bp.pad, k = bpTabKey();
    const back = `<button type="button" class="ph-btn sec sm ph-press ph-bpback" data-bp="back" data-key="back">${pad ? glyph('b') : ic('back', 's')}${esc(t('Back'))}</button>`;
    const n = dlIds().length;
    const tabs = `<div class="ph-bptabs" data-key="tabs">${pad ? glyph('lb') : ''}<div class="in"><span class="th"></span>${BP_TABS.map(([key, name], i) => `<button type="button" class="${i === bp.tab ? 'on' : ''}" data-bp="tab" data-i="${i}" data-key="t:${i}" aria-label="${esc(t(name))}">${ic(BP_ICON[key], 's')}<span>${esc(t(name))}</span>${key === 'downloads' && n ? `<em>${n}</em>` : ''}</button>`).join('')}</div>${pad ? glyph('rb') : ''}</div>`;
    const hints = pad ? `<div class="ph-bphints" data-key="hints">${[['a', t('Select')], ['b', t('Back')], ['y', t('Options')]].map(([g, l]) => `<span>${glyph(g)}${esc(l)}</span>`).join('')}</div>` : '';
    const top = `<div class="ph-bptop" data-key="top">${bp.menu || bp.game ? back : tabs}<span class="ph-grow"></span>${hints}<button type="button" class="ph-ibtn glass ph-press" data-bp="exit" data-key="exit" aria-label="${esc(t('Leave Big Picture mode'))}">${ic('x', 's')}</button></div>`;
    let body;
    if (bp.menu && appOf(bp.menu)) body = bpMenuHTML(appOf(bp.menu));
    else if (bp.game && appOf(bp.game)) body = bpGameHTML(appOf(bp.game));
    else if (k === 'home') body = bpHome();
    else if (k === 'downloads') body = bpDownloads();
    else if (k === 'ach') body = bpAch();
    else if (k === 'settings') body = bpSettings();
    else body = bpGridHTML();
    return top + body;
  }
  // The focused game's art fades in over the last one.
  function bpBg(a) {
    const box = el.bpbg, u = a ? pageArt(a) : '';
    if (!box || box.dataset.u === u) return;
    box.dataset.u = u;
    if (!u) return;
    const pic = document.createElement('img');
    pic.alt = '';
    pic.decoding = 'async';
    if (!a.hero) pic.className = 'blur';
    pic.src = u;
    box.appendChild(pic);
    while (box.children.length > 2) box.firstChild.remove();
  }
  // The section pill's light slides to the section picked, whatever its width.
  function bpThumb() {
    const tabs = $('.ph-bptabs .in', el.bpin), on = tabs && $('button.on', tabs), th = tabs && $('.th', tabs);
    if (!on || !th) return;
    th.style.transform = `translateX(${on.offsetLeft - 4}px)`;
    th.style.width = on.offsetWidth + 'px';
  }
  function bpDraw() {
    if (!bp || !el.bpin || bp.busy) return;
    // What players say about each game (once an hour at most), for the score under its name.
    wantCounts();
    const k = bpTabKey(), art = bp.menu || bp.game || (k === 'home' ? bp.focus : '');
    paint(el.bpin, bpHTML());
    el.bp.classList.toggle('dim', !art || !!bp.menu || bp.low);
    el.bp.classList.toggle('pad', bp.pad);
    root.classList.toggle('ph-bppad', bp.pad);
    bpBg(appOf(art) || appOf(bp.focus));
    bpThumb();
    bpLight(false);
  }
  // Scrolled down Home or a game's page: the art behind dims, so the rows and words read.
  function bpScrolled(n) {
    const low = n.scrollTop > n.clientHeight * (n.classList.contains('home') ? 0.35 : 0.25);
    if (low === !!bp.low) return;
    bp.low = low;
    el.bp.classList.toggle('dim', low || !!bp.menu || !(bp.game || bpTabKey() === 'home'));
  }
  // ---------- moving around with a controller ----------
  // In the sheet when one is up (a README, the reviews, writing one), else on Big Picture's page.
  const bpScope = () => (P.sheet ? el.sheet : el.bpin);
  const bpItems = () => { const sc = bpScope(); return sc ? [...sc.querySelectorAll('button:not(:disabled),input,[data-f]')].filter((b) => b.offsetParent && !b.closest('[hidden]')) : []; };
  const bpKey = (b) => b.dataset.key || [b.dataset.act || b.dataset.bp || (b.dataset.menu ? 'menu' : '') || (b.dataset.sw ? 'sw' : '') || (b.dataset.readme ? 'readme' : ''), b.dataset.id || b.dataset.menu || b.dataset.sw || b.dataset.i || b.dataset.v || b.dataset.readme || ''].join(':');
  function bpFirst(all) {
    const pick = (sel) => all.find((b) => b.matches(sel));
    if (P.sheet) return pick('.ph-rvpick') || pick('button');
    if (bp.menu) return pick('.ph-bprow');
    if (bp.game) return pick('.ph-cta button');
    const k = bpTabKey();
    if (k === 'home') return pick('.ph-bpshelf .f') || pick('.ph-cta button');
    if (k === 'catalog') return pick('.ph-bpgrid button') || pick('.ph-bpchips2 .on');
    return pick('.ph-bpgrid button,.ph-bpset .ph-bprow button,.ph-bpset button.ph-bprow,.ph-bplist button,.ph-bplist [data-f]') || pick('.ph-bptabs .on');
  }
  function bpLight(scroll) {
    const all = bpItems(), sheetOn = !!P.sheet;
    const want = sheetOn ? bp.sf : bp.f;
    let f = all.find((b) => bpKey(b) === want);
    // A game's buttons change under the highlight (Install becomes the download's Cancel): it stays on them.
    if (!f && !sheetOn && bp.cta) f = all.find((b) => b.closest('.ph-cta'));
    if (!f) f = bpFirst(all);
    if (sheetOn) bp.sf = f ? bpKey(f) : '';
    else { bp.f = f ? bpKey(f) : ''; bp.cta = !!(f && f.closest('.ph-cta')); }
    for (const b of root.querySelectorAll('.ph-bpon')) if (b !== f) b.classList.remove('ph-bpon');
    if (f) f.classList.add('ph-bpon');
    if (scroll && f) bpShow(f);
  }
  function bpShow(b) {
    const how = still() ? 'auto' : 'smooth';
    const line = b.closest('.ph-bpshelf,.ph-bpline');
    if (line) line.scrollTo({ left: b.offsetLeft - (line.clientWidth - b.offsetWidth) / 2, behavior: how });
    const sc = b.closest('.ph-bpscroll');
    if (sc && b.closest('.ph-bphero')) return sc.scrollTo({ top: 0, behavior: how });
    if (sc && b.closest('.ph-bpgtop')) return sc.scrollTo({ top: 0, behavior: how });
    const sec = b.closest('.ph-bpsec');
    if (sec) return sec.scrollIntoView({ block: 'center', behavior: how });
    if (!line) b.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior: how });
  }
  function bpFocus(b) {
    if (P.sheet) bp.sf = bpKey(b); else bp.f = bpKey(b);
    // On Home, the shelf's cover in focus is the game shown above it.
    if (!P.sheet && b.dataset.bp === 'focus' && b.dataset.id !== bp.focus) { bp.focus = b.dataset.id; bpDraw(); }
    else bpLight(false);
    bpShow(b);
  }
  // The nearest thing that way. Coming into a row or the tabs lands on the one picked there. With
  // nothing more that way, a long page (a README, a game's page) scrolls instead.
  function bpMove(dir) {
    const all = bpItems(), cur = all.find((b) => b.classList.contains('ph-bpon'));
    if (!cur) return bpLight(true);
    const r = cur.getBoundingClientRect(), cx = r.left + r.width / 2, cy = r.top + r.height / 2;
    const flat = dir === 'left' || dir === 'right';
    let best = null, score = Infinity;
    for (const b of all) {
      if (b === cur) continue;
      const q = b.getBoundingClientRect();
      const dx = q.left + q.width / 2 - cx, dy = q.top + q.height / 2 - cy;
      const ahead = dir === 'left' ? -dx : dir === 'right' ? dx : dir === 'up' ? -dy : dy;
      if (ahead <= 4) continue;
      // A row's neighbours come before anything further off to the side.
      const off = flat ? Math.max(0, Math.abs(dy) - (q.height + r.height) / 4) : Math.max(0, Math.abs(dx) - (q.width + r.width) / 4);
      const s = ahead + off * 3;
      if (s < score) { score = s; best = b; }
    }
    // Further than a screen away, or nothing at all: a long page scrolls a step first.
    const sc = cur.closest('.ph-bpscroll,.ph-sheet,.rows');
    const far = best && sc && !flat && Math.abs(best.getBoundingClientRect().top - r.top) > sc.getBoundingClientRect().height * 0.9;
    if ((!best || far) && sc && !flat && sc.scrollHeight > sc.clientHeight + 2) {
      const before = sc.scrollTop;
      sc.scrollBy({ top: (dir === 'down' ? 1 : -1) * sc.clientHeight * 0.6, behavior: still() ? 'auto' : 'smooth' });
      if (sc.scrollTop !== before || !still()) { if (!best || (dir === 'down' ? sc.scrollTop + sc.clientHeight < sc.scrollHeight - 2 : sc.scrollTop > 2)) return; }
    }
    if (!best) return;
    for (const sel of ['.ph-bpshelf', '.ph-bptabs', '.ph-bpline', '.ph-bpchips2']) {
      const box = best.closest(sel);
      if (box && !cur.closest(sel)) best = $(sel === '.ph-bptabs' || sel === '.ph-bpchips2' ? '.on' : '.f', box) || best;
    }
    bpFocus(best);
  }
  // ---------- the sections and pages ----------
  function bpTab(i) {
    if (i === bp.tab && !bp.game && !bp.menu) return;
    Object.assign(bp, { tab: i, game: '', menu: '', f: '', n: BP_STEP, low: false });
    buzz();
    bpDraw();
    const sc = $('.ph-bpscroll', el.bpin);
    if (sc) sc.scrollTop = 0;
  }
  function bpGame(id) {
    if (!appOf(id)) return;
    const sc = $('.ph-bpscroll', el.bpin);
    bp.back = { f: bp.menu ? bp.mback : bp.f, y: sc && !bp.game ? sc.scrollTop : 0, game: bp.game };
    Object.assign(bp, { game: id, menu: '', f: '', low: false });
    buzz();
    bpDraw();
    const g = $('.ph-bpscroll', el.bpin);
    if (g) g.scrollTop = 0;
  }
  function bpMenu(id) {
    if (!appOf(id)) return;
    bp.mback = bp.f;
    Object.assign(bp, { menu: id, f: '' });
    buzz();
    bpDraw();
  }
  // Back a step: a sheet, the menu, then a game's page, then to Home. False on Home itself.
  function bpBack() {
    if (!bp) return false;
    if (P.sheet) { closeSheet(); return true; }
    if (document.activeElement && document.activeElement.matches && document.activeElement.matches('input,textarea')) { document.activeElement.blur(); return true; }
    if (bp.menu) { Object.assign(bp, { menu: '', f: bp.mback || '' }); bpDraw(); return true; }
    if (bp.game) {
      const b = bp.back || {};
      Object.assign(bp, { game: b.game || '', f: b.f || '', low: false, back: null });
      bpDraw();
      const sc = $('.ph-bpscroll', el.bpin);
      if (sc && b.y) sc.scrollTop = b.y;
      return true;
    }
    if (bp.tab !== 0) { bpTab(0); return true; }
    return false;
  }
  async function bpAct(b) {
    const d = b.dataset;
    switch (d.bp) {
      case 'tab': return bpTab(Number(d.i));
      case 'exit': return bpLeave();
      case 'back': return bpBack();
      case 'game': return bpGame(d.id);
      case 'con': bp.con = d.k || ''; bp.n = BP_STEP; buzz(); return bpDraw();
      case 'glyphs': {
        const i = GLYPHS.findIndex(([k]) => k === (ui().tv_glyphs || ''));
        buzz();
        await saveUi({ tv_glyphs: GLYPHS[(i + 1) % GLYPHS.length][0] || null }, true);
        return bpDraw();
      }
      // A cover on the shelf: the first tap shows it, the next opens its page.
      case 'focus': if (d.id === bp.focus) return bpGame(d.id); bp.f = bpKey(b); bp.focus = d.id; buzz(); bpDraw(); return bpShow(b);
    }
  }
  // A controller's button (from tv.js) or a key. True: Big Picture took it.
  function bpPad(b, kind) {
    if (!bp) return false;
    if (bp.busy) return true;
    const was = bp.pad, k = S.padKind;
    bp.pad = true;
    if (!was || (kind && kind !== bp.kind)) { bp.kind = kind || k; bpDraw(); }
    const cur = bpItems().find((x) => x.classList.contains('ph-bpon'));
    switch (b) {
      case 'up': case 'down': case 'left': case 'right': bpMove(b); break;
      case 'a':
        if (!cur) { bpLight(true); break; }
        if (cur.matches('input')) { cur.focus(); break; }
        cur.click();
        break;
      case 'b': bpBack(); break;
      case 'x': case 'y': {
        if (P.sheet) break;
        const id = bp.menu ? '' : (cur && cur.dataset.id && (cur.dataset.bp === 'focus' || cur.dataset.bp === 'game') ? cur.dataset.id : bp.game || (bpTabKey() === 'home' ? bp.focus : ''));
        if (bp.menu) bpBack(); else if (id) bpMenu(id);
        break;
      }
      case 'lb': case 'rb': if (!bp.game && !bp.menu && !P.sheet) bpTab((bp.tab + (b === 'rb' ? 1 : BP_TABS.length - 1)) % BP_TABS.length); break;
      case 'menu': if (P.sheet) closeSheet(); bpTab(bpTabOf('settings')); break;
    }
    return true;
  }
  // The next covers of a long grid, once it's scrolled near its end.
  function bpMore(n) {
    if (!bp || n.scrollTop + n.clientHeight < n.scrollHeight - n.clientHeight) return;
    const k = bpTabKey();
    if (bp.game || (k !== 'library' && k !== 'catalog') || bp.n >= bpGrid().length) return;
    bp.n += BP_STEP;
    bpDraw();
  }
  // ---------- in and out ----------
  // Turning the screen happens behind a dark cover: it fades in, the window turns (or a moment passes
  // with nothing to turn) and settles at its new size, and only then is the next layout drawn and
  // shown. Nothing is ever seen half-turned or drawn for the wrong shape.
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  function settle(landscape, idle) {
    return new Promise((done) => {
      const t0 = performance.now();
      let w = innerWidth, h = innerHeight, moved = 0;
      const right = () => landscape == null || (landscape ? innerWidth > innerHeight : innerWidth <= innerHeight);
      const step = (now) => {
        if (innerWidth !== w || innerHeight !== h) { w = innerWidth; h = innerHeight; moved = now; }
        // After a turn, a short quiet (the bars can still come or go); with no turn, a little longer.
        const quiet = now - (moved || t0) > (moved ? 160 : idle);
        if ((quiet && right()) || now - t0 > 1600) return done();
        requestAnimationFrame(step);
      };
      requestAnimationFrame(step);
    });
  }
  window.screenSettle = settle;
  async function bpEnter(byPad) {
    if (bp) return;
    if (!P.on) enter();
    if (P.sheet) closeSheet();
    if (P.search) closeSearch();
    if (P.stack.length) closeGame(true);
    closeSub(true);
    bp = { tab: 0, focus: '', game: '', menu: '', f: '', sf: '', con: '', q: '', n: BP_STEP, pad: !!byPad || padHere(), byPad: !!byPad, kind: S.padKind, busy: 'in' };
    const box = document.createElement('div');
    box.className = 'ph-bp cover';
    box.innerHTML = '<div class="ph-bpbg"></div><i class="sc1"></i><i class="sc2"></i><div class="ph-bpin"></div>';
    // Under the start-up screen, when it's still up.
    el.ui.insertBefore(box, el.boot);
    Object.assign(el, { bp: box, bpbg: box.firstChild, bpin: box.lastChild });
    buzz();
    if (!still()) await wait(200);
    if (!bp || el.bp !== box) return;
    root.classList.add('ph-bpon');
    // Sideways, with the system's bars hidden, behind the cover.
    invoke('android_orientation', { landscape: true }).catch(() => {});
    invoke('android_edges', { phone: false, light: false }).catch(() => {});
    if (S.os === 'android') await settle(true, 220);
    if (!bp || el.bp !== box) return;
    await syncInsets();
    bp.busy = '';
    box.classList.remove('cover');
    bpDraw();
  }
  async function bpLeave() {
    if (!bp || bp.busy === 'out') return;
    const box = el.bp;
    bp.busy = 'out';
    if (P.sheet) closeSheet();
    // Big Picture's own things fade first; the dark stays while the phone turns back.
    box.classList.add('cover', 'out');
    if (!still()) await wait(190);
    bp = null;
    el.bp = el.bpbg = el.bpin = null;
    box.lastChild.innerHTML = '';
    invoke('android_orientation', { landscape: false }).catch(() => {});
    // The bars back as the phone layout has them (applyLook sets them when the theme is new to it).
    delete root.dataset.theme;
    applyLook();
    if (S.os === 'android') await settle(null, 480);
    await syncInsets();
    refresh();
    root.classList.remove('ph-bpon');
    if (still() || !box.animate) return box.remove();
    box.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 260, easing: 'ease-out', fill: 'forwards' }).onfinish = () => box.remove();
  }
  // Big Picture style changed from here: Phone stays; Full (the big-screen one, tv.js) comes up over
  // this one, which then goes without turning the phone back first.
  async function bpStyleSwap() {
    const full = pref.bpStyle() !== 'full';
    buzz();
    await save('bp_style', full ? 'full' : 'phone');
    if (!bp || !full) return bp ? bpDraw() : drawView('settings');
    const box = el.bp;
    bp.busy = 'out';
    await toTV();
    bp = null;
    el.bp = el.bpbg = el.bpin = null;
    if (box) box.remove();
    root.classList.remove('ph-bpon');
  }
  // The last controller gone: back to the phone layout, if a controller is what opened it.
  function bpPadGone() {
    if (bp && bp.byPad && !S.settings.start_in_tv) bpLeave();
  }
  // Android's Back: a step back, and from Home out of Big Picture (or, when it's how the launcher
  // opens, out of the way like any app).
  function bpSysBack() {
    if (bpBack()) return;
    if (S.settings.start_in_tv) invoke('android_to_back').catch(() => {});
    else bpLeave();
  }
  window.phoneBp = { enter: bpEnter, leave: bpLeave, on: () => !!bp, pad: bpPad, padGone: bpPadGone, back: bpSysBack };
  // A keyboard works as a controller: arrows, Enter, Escape, Q and E for the sections.
  addEventListener('keydown', (e) => {
    if (!bp || e.ctrlKey || e.altKey || e.metaKey || (e.target.closest && e.target.closest('input,textarea'))) return;
    const n = { ArrowUp: 'up', ArrowDown: 'down', ArrowLeft: 'left', ArrowRight: 'right', Enter: 'a', ' ': 'a', Escape: 'b', Backspace: 'b', q: 'lb', e: 'rb', PageUp: 'lb', PageDown: 'rb', x: 'x', y: 'y' }[e.key];
    if (!n) return;
    e.preventDefault();
    e.stopPropagation();
    bpPad(n);
  }, true);

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
    bp = null;
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
