// Plugins: their pages, their places in the sidebar and game menus, Settings → Plugins, and the
// requests their pages send. A plugin's page runs in a sandboxed frame with no way into the
// launcher; it asks through postMessage, and this file checks and answers. Anything that
// downloads, installs or adds to the Library asks the player first, and so does the first
// connection to each site.
'use strict';

S.plugins = { all_off: false, plugins: [] };
const PLUG_NOTE = N_('Plugins add pages and tools made by other people. Each one runs walled off from the rest of the launcher, can only do what it asked for when you installed it, and asks you before it downloads or installs anything and the first time it connects to a site.');
const PERM_WORDS = {
  'library.read': N_('See the games in your Library'),
  'library.add': N_('Add folders to your Library (asks you each time)'),
  network: N_('Connect to {sites} (asks you the first time for each)'),
  install: N_('Download and install games from {sites} (asks you each time)'),
  notify: N_('Show notifications in the launcher'),
};

const plugOn = () => (S.plugins.all_off ? [] : S.plugins.plugins.filter((p) => p.enabled));
const plugMan = (id) => (S.plugins.plugins.find((p) => p.manifest.id === id) || {}).manifest;
const plugCan = (m, perm) => (m.permissions || []).includes(perm);
// A plugin file's address: rlplug:// (http://rlplug.localhost on Windows).
const plugUrl = (id, rel) => (S.os === 'windows' ? 'http://rlplug.localhost/' : 'rlplug://localhost/') + [id, ...String(rel).split('/')].map(encodeURIComponent).join('/');
const PUZZLE = '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M10 4.5a2 2 0 0 1 4 0V6h4a1 1 0 0 1 1 1v4h-1.5a2 2 0 0 0 0 4H19v4a1 1 0 0 1-1 1h-4v-1.5a2 2 0 0 0-4 0V20H6a1 1 0 0 1-1-1v-4h1.5a2 2 0 0 0 0-4H5V7a1 1 0 0 1 1-1h4z"/></svg>';
const plugIcon = (m, file) => (file ? `<img class="plug-ic" src="${esc(plugUrl(m.id, file))}" alt="" width="20" height="20">` : PUZZLE);

async function loadPlugins() {
  try { S.plugins = await invoke('plugin_list'); } catch (e) { S.plugins = { all_off: false, plugins: [] }; }
  syncPluginNav();
  if (S.view === 'settings' || S.view.startsWith('plug:')) {
    // A plugin that was turned off takes its page with it.
    if (S.view.startsWith('plug:') && !plugOn().some((p) => p.manifest.id === S.view.slice(5))) return go('home');
    render(true);
  }
}

// Each plugin with a page gets a place in the sidebar, which the player can move or hide like the rest.
function syncPluginNav() {
  const nav = $('#side .navs');
  if (!nav) return;
  const want = plugOn().filter((p) => p.manifest.page);
  for (const b of $$('.nav[data-plug]', nav)) if (!want.some((p) => p.manifest.id === b.dataset.plug)) b.remove();
  for (const p of want) {
    const m = p.manifest, title = (m.page.title || m.name).slice(0, 24);
    let b = $(`.nav[data-plug="${CSS.escape(m.id)}"]`, nav);
    if (!b) {
      b = document.createElement('button');
      b.type = 'button';
      b.className = 'nav';
      b.dataset.plug = m.id;
      b.dataset.go = 'plug:' + m.id;
      b.dataset.i = 50 + $$('.nav[data-plug]', nav).length;
      nav.append(b);
    }
    const html = `${plugIcon(m, m.page.icon || m.icon)}<span class="lbl">${esc(title)}</span>`;
    if (b.dataset.html !== html) { b.innerHTML = html; b.dataset.html = html; }
  }
  renderSide();
}

window.pagePlugin = (id) => {
  const m = plugOn().map((p) => p.manifest).find((x) => x.id === id && x.page);
  if (!m) return `<div class="empty"><p>${esc(t('This plugin is off.'))}</p></div>`;
  // No allow-same-origin: the page gets an empty origin of its own and can't reach the launcher.
  return `<div class="plug-page"><iframe class="plug-frame" data-id="${esc(id)}" title="${esc(m.name)}" sandbox="allow-scripts allow-forms" referrerpolicy="no-referrer" src="${esc(plugUrl(id, m.page.entry || 'index.html'))}"></iframe></div>`;
};

// Game menu items from plugins.
window.pluginMenu = (gameId) => {
  const items = [];
  for (const p of plugOn()) for (const it of p.manifest.menu || []) items.push({ act: 'plug-menu', id: gameId, k: p.manifest.id + '\n' + it.id, label: it.label, icon: 'puzzle' });
  return items.length ? ['-', ...items] : [];
};

// What a plugin page is waiting to hear once it's ready (a menu item picked before it was open).
const plugPending = {};
function plugSend(id, msg) {
  const f = $$('iframe.plug-frame').find((x) => x.dataset.id === id);
  if (f && f.contentWindow) f.contentWindow.postMessage(Object.assign({ rl: 1 }, msg), '*');
}
async function plugMenuPicked(gameId, key) {
  const [pid, item] = key.split('\n');
  const m = plugMan(pid);
  if (!m) return;
  const g = S.library[gameId], x = byId(gameId);
  const game = { id: gameId, name: (g && g.name) || (x && x.name) || gameId };
  if (g && plugCan(m, 'library.read')) game.folder = g.path;
  const msg = { event: 'menu', data: { item, game } };
  if (S.view === 'plug:' + pid) return plugSend(pid, msg);
  plugPending[pid] = msg;
  go('plug:' + pid);
}

// ---------- asking the player ----------

// One question at a time, whichever plugin asks.
let plugAsking = Promise.resolve();
function plugAsk(build) {
  const next = plugAsking.then(() => new Promise((done) => {
    const { html, buttons } = build();
    const s = modal(`${html}<div class="acts grid${buttons.length}">${buttons.map(([k, label, main]) => `<button type="button" class="btn${main ? ' main' : ''}" data-x="${k}">${esc(label)}</button>`).join('')}</div>`, (x, close) => { close(); done(x.dataset.x); }, t('A plugin is asking'));
    // Closed another way (Escape, outside): no.
    new MutationObserver((_, o) => { if (!s.isConnected) { o.disconnect(); done('no'); } }).observe(document.body, { childList: true });
  }));
  plugAsking = next.catch(() => {});
  return next;
}
const plugHead = (m) => `<div class="plug-ask-h">${plugIcon(m, m.icon)}<b>${esc(m.name)}</b></div>`;

function askSite(m, host) {
  return plugAsk(() => ({
    html: `${plugHead(m)}<h2>${esc(tf('Let {name} connect to {site}?', { name: m.name, site: host }))}</h2><p>${esc(t('It sends and gets data from that site through the launcher. Nothing from your other sites or your sign-in goes along.'))}</p>`,
    buttons: [['no', t('Don\'t Allow')], ['once', t('Allow Once')], ['always', t('Always Allow This Site'), true]],
  }));
}

// ---------- requests from plugin pages ----------

// A plugin request, asking the player about each site it reaches that they haven't been asked about
// (a site's own download server after a redirect, say).
async function withSites(m, cmd, args) {
  for (let tries = 0; tries < 4; tries++) {
    try { return await invoke(cmd, args); } catch (e) {
      const msg = String(e && e.message ? e.message : e);
      if (!msg.startsWith('ASK_SITE|')) throw e;
      const host = msg.slice(9);
      const ans = await askSite(m, host);
      if (ans === 'no') throw new Error(tf('You didn\'t allow {site}.', { site: host }));
      await invoke('plugin_allow_site', { id: m.id, host, always: ans === 'always' });
    }
  }
  throw new Error('Not allowed.');
}
function plugFetch(m, a) {
  return withSites(m, 'plugin_fetch', { id: m.id, url: String(a.url || ''), method: a.method || null, headers: a.headers || null, body: a.body == null ? null : String(a.body), binary: !!a.binary });
}

let plugToastAt = 0;
const PLUG_CALLS = {
  ready: (m) => { const p = plugPending[m.id]; if (p) { delete plugPending[m.id]; setTimeout(() => plugSend(m.id, p), 0); } return true; },
  info: () => ({ launcher: S.version, os: S.os, language: document.documentElement.lang || 'en', theme: (S.settings.ui || {}).theme || 'dark' }),
  'library.list': (m) => invoke('plugin_library', { id: m.id }),
  'library.addFolder': async (m, a) => {
    if (!plugCan(m, 'library.add')) throw new Error(t('It didn\'t ask for permission to do that.'));
    const folder = String(a.folder || ''), name = String(a.name || '').slice(0, 80);
    const ans = await plugAsk(() => ({
      html: `${plugHead(m)}<h2>${esc(tf('Add {name} to your Library?', { name: name || t('this folder') }))}</h2><p>${esc(tf('{plugin} wants to add this folder as a game:', { plugin: m.name }))}</p><p class="path">${esc(folder)}</p>`,
      buttons: [['no', t('Cancel')], ['yes', t('Add to Library'), true]],
    }));
    if (ans !== 'yes') throw new Error(t('You said no.'));
    const g = await invoke('plugin_add_folder', { id: m.id, folder, name: name || null });
    await refreshState();
    return { id: g.id, name: g.name };
  },
  'net.fetch': (m, a) => plugFetch(m, a),
  'install.zip': async (m, a) => {
    if (!plugCan(m, 'install')) throw new Error(t('It didn\'t ask for permission to do that.'));
    const url = String(a.url || ''), name = String(a.name || '').trim().slice(0, 80);
    let host = '', file = '';
    try { const u = new URL(url); host = u.hostname; file = decodeURIComponent(u.pathname.split('/').pop() || ''); } catch (e) { throw new Error(t('That isn\'t an address.')); }
    const ans = await plugAsk(() => ({
      html: `${plugHead(m)}<h2>${esc(tf('Download and install {name}?', { name }))}</h2><p>${esc(tf('{plugin} wants to download {file} from {site} and put it in your Library as a game.', { plugin: m.name, file, site: host }))}</p><p class="muted">${esc(t('Only say yes if you trust this plugin and where the file comes from.'))}</p>`,
      buttons: [['no', t('Cancel')], ['yes', t('Download and Install'), true]],
    }));
    if (ans !== 'yes') throw new Error(t('You said no.'));
    await invoke('plugin_allow_site', { id: m.id, host, always: false });
    toast(tf('Downloading {name}…', { name }));
    const g = await withSites(m, 'plugin_install_zip', { id: m.id, url, name, headers: a.headers && typeof a.headers === 'object' ? a.headers : null });
    await refreshState();
    toast(tf('{name} is in your Library.', { name: g.name }));
    return { id: g.id, name: g.name };
  },
  'storage.get': async (m, a) => { const all = await invoke('plugin_store_get', { id: m.id }); return all[String(a.key)] ?? null; },
  'storage.all': (m) => invoke('plugin_store_get', { id: m.id }),
  'storage.set': (m, a) => invoke('plugin_store_set', { id: m.id, key: String(a.key), value: a.value === undefined ? null : a.value }),
  notify: (m, a) => {
    if (!plugCan(m, 'notify')) throw new Error(t('It didn\'t ask for permission to do that.'));
    toast(`${m.name}: ${String(a.title || '').slice(0, 80)}${a.body ? ' · ' + String(a.body).slice(0, 160) : ''}`);
    return true;
  },
  toast: (m, a) => {
    // From the page the player has open, now and then.
    if (S.view !== 'plug:' + m.id || Date.now() - plugToastAt < 1500) return false;
    plugToastAt = Date.now();
    toast(String(a.text || '').slice(0, 160));
    return true;
  },
  'game.open': (m, a) => { if (!plugCan(m, 'library.read')) throw new Error(t('It didn\'t ask for permission to do that.')); go('game', String(a.id)); return true; },
};

addEventListener('message', async (e) => {
  const d = e.data;
  if (!d || d.rl !== 1 || !d.call) return;
  // Only from a plugin frame, and answered only to it.
  const f = $$('iframe.plug-frame').find((x) => x.contentWindow === e.source);
  if (!f) return;
  const m = plugOn().map((p) => p.manifest).find((x) => x.id === f.dataset.id);
  const reply = (ok, v) => { try { e.source.postMessage({ rl: 1, n: d.n, ok, [ok ? 'value' : 'error']: v }, '*'); } catch (err) { /* gone */ } };
  if (!m) return reply(false, 'This plugin is off.');
  const fn = Object.prototype.hasOwnProperty.call(PLUG_CALLS, d.call) ? PLUG_CALLS[d.call] : null;
  if (!fn) return reply(false, 'Unknown request: ' + String(d.call).slice(0, 40));
  try { reply(true, await fn(m, d.args && typeof d.args === 'object' ? d.args : {})); } catch (err) { reply(false, String(err && err.message ? err.message : err)); }
});

// ---------- Settings → Plugins ----------

function permLines(m) {
  const sites = (m.sites || []).join(', ');
  const lines = (m.permissions || []).map((p) => tf(PERM_WORDS[p] || p, { sites }));
  lines.push(t('Keep its own settings'));
  return lines;
}

window.pluginSettings = (group, row, sw) => {
  const list = S.plugins.plugins;
  const head = `<button type="button" class="btn sm main" data-act="plug-add">${esc(t('Install From File…'))}</button>`;
  const cards = list.map((p) => {
    const m = p.manifest;
    const by = [m.version && tf('Version {v}', { v: m.version }), m.author && tf('by {name}', { name: m.author })].filter(Boolean).join(' · ');
    const sites = (p.sites || []).map((h) => `<span class="plug-chip">${esc(h)}<button type="button" class="chip-x" data-act="plug-forget" data-id="${esc(m.id)}" data-k="${esc(h)}" aria-label="${esc(tf('Stop always allowing {site}', { site: h }))}">×</button></span>`).join('');
    return `<div class="set-row plug-row"><div class="plug-ic-big">${plugIcon(m, m.icon)}</div><div class="k"><b>${esc(m.name)}</b><span>${esc(by)}</span>${m.description ? `<span>${esc(m.description)}</span>` : ''}
        <ul class="plug-perms">${permLines(m).map((l) => `<li>${esc(l)}</li>`).join('')}</ul>${sites ? `<div class="plug-sites"><span>${esc(t('Always allowed:'))}</span>${sites}</div>` : ''}</div>
      <span class="acts">${m.page && p.enabled && !S.plugins.all_off ? `<button type="button" class="btn sm" data-go="plug:${esc(m.id)}">${esc(t('Open'))}</button>` : ''}
        <button type="button" class="icon-btn" data-act="plug-remove" data-id="${esc(m.id)}" aria-label="${esc(tf('Uninstall {name}', { name: m.name }))}" title="${esc(t('Uninstall'))}"><svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3"/></svg></button>
        <button type="button" class="switch" role="switch" aria-checked="${!!p.enabled}" aria-label="${esc(tf('Turn on {name}', { name: m.name }))}" data-act="plug-enable" data-id="${esc(m.id)}"></button></span></div>`;
  }).join('');
  return group(t('Plugins'), `<p class="set-note">${esc(t(PLUG_NOTE))}</p>` + (cards || `<p class="set-foot">${esc(t('No plugins yet. Install one from its .rlplugin file.'))}</p>`), head)
    + group(t('Safety'), row(t('Turn Off Every Plugin'), t('Stops all plugins at once, without uninstalling them.'), sw('all_off', S.plugins.all_off, t('Turn Off Every Plugin'), 'plug-alloff'))
      + row(t('Making a Plugin'), t('The developer guide shows how plugins are made and what they can ask for.'), `<button type="button" class="btn sm" data-url="https://github.com/elripalda/RecompLauncher/blob/main/docs/PLUGINS.md">${esc(t('Read the Guide'))}</button>`));
};

async function installPlugin() {
  const p = await pickFile({ title: t('A plugin file'), directory: false, multiple: false, filters: [{ name: t('RecompLauncher plugins'), extensions: ['rlplugin', 'zip'] }] }).catch(fail);
  if (!p) return;
  const path = typeof p === 'string' ? p : p.path;
  let info;
  try { info = await invoke('plugin_inspect', { path }); } catch (e) { return fail(e); }
  const m = info.manifest;
  const title = info.installed ? tf('Update {name} from {from} to {to}?', { name: m.name, from: info.installed, to: m.version }) : tf('Install {name}?', { name: m.name });
  modal(`${plugHead(m)}<h2>${esc(title)}</h2>${m.author ? `<p class="muted">${esc(tf('by {name}', { name: m.author }))}</p>` : ''}${m.description ? `<p>${esc(m.description)}</p>` : ''}
    <p><b>${esc(t('It will be able to:'))}</b></p><ul class="plug-perms">${permLines(m).map((l) => `<li>${esc(l)}</li>`).join('')}</ul>
    <p class="muted">${esc(t('Only install plugins from people you trust.'))}</p>
    <div class="acts grid2"><button type="button" class="btn" data-x="no">${esc(t('Cancel'))}</button><button type="button" class="btn main" data-x="yes">${esc(info.installed ? t('Update') : t('Install'))}</button></div>`, async (x, close) => {
    if (x.dataset.x !== 'yes') return close();
    close();
    try {
      await invoke('plugin_install', { path });
      if (!info.installed) await invoke('plugin_enable', { id: m.id, on: true });
      toast(info.installed ? tf('{name} is updated.', { name: m.name }) : tf('{name} is installed and on.', { name: m.name }));
    } catch (e) { fail(e); }
    loadPlugins();
  }, t('Install a plugin'));
}

function removePlugin(id) {
  const m = plugMan(id);
  if (!m) return;
  modal(`${plugHead(m)}<h2>${esc(tf('Uninstall {name}?', { name: m.name }))}</h2><p>${esc(t('Its files and the settings it kept go too. Games it put in your Library stay.'))}</p>
    <div class="acts grid2"><button type="button" class="btn" data-x="no">${esc(t('Cancel'))}</button><button type="button" class="btn danger" data-x="yes">${esc(t('Uninstall'))}</button></div>`, async (x, close) => {
    close();
    if (x.dataset.x !== 'yes') return;
    try { await invoke('plugin_uninstall', { id }); toast(tf('{name} is uninstalled.', { name: m.name })); } catch (e) { fail(e); }
    if (S.view === 'plug:' + id) go('home');
    loadPlugins();
  }, t('Uninstall a plugin'));
}

// Clicks on plugin controls (before app.js's own handler sees them).
document.addEventListener('click', async (e) => {
  const el = e.target.closest('[data-act^="plug-"]');
  if (!el) return;
  e.stopPropagation();
  if (el.closest('.menu')) closeMenu();
  const id = el.dataset.id;
  try {
    switch (el.dataset.act) {
      case 'plug-add': return installPlugin();
      case 'plug-remove': return removePlugin(id);
      case 'plug-enable': { const p = S.plugins.plugins.find((x) => x.manifest.id === id); await invoke('plugin_enable', { id, on: !(p && p.enabled) }); return loadPlugins(); }
      case 'plug-alloff': await invoke('plugin_all_off', { off: !S.plugins.all_off }); return loadPlugins();
      case 'plug-forget': await invoke('plugin_forget_site', { id, host: el.dataset.k }); return loadPlugins();
      case 'plug-menu': return plugMenuPicked(id, el.dataset.k);
    }
  } catch (err) { fail(err); }
}, true);

loadPlugins();
