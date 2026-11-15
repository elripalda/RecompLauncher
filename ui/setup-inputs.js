// Named Easy Setup inputs. Shared reuse is automatic; requirements come only from the profile.
// Kept separate from the single-copy flow so existing profiles retain their setup behavior.

// A game's own media on the setup screen (media-art.js): its console's cartridge, card or disc
// with the game on it, as the site's media design for it says (the catalog's media_design; a
// game's media is its "Around the web" links). state: 'ghost' (waiting for the player's files),
// 'is-in' (arriving), 'is-busy' (working), 'is-done' or ''.
function gameMediaArt(a) {
  return { cover: a.cover ? media(thumbOf(a.cover, 600)) : '', hero: a.hero ? media(thumbOf(a.hero, 960)) : '', logo: a.logo ? media(a.logo) : '', design: a.media_design && typeof a.media_design === 'object' && !Array.isArray(a.media_design) ? a.media_design : null };
}
function gameMediaHtml(a, state) {
  const kind = MediaArt.kindOf(a.origin);
  if (state === 'ghost') return MediaArt.ghost(kind);
  return MediaArt.render(kind, gameMediaArt(a)).replace('<div class="md ', `<div class="md ${state || ''} `);
}
function mediaState(m, state) {
  if (!m || !m.classList.contains('md') || m.classList.contains('md-ghost')) return;
  m.classList.remove('is-in', 'is-busy', 'is-done');
  if (state) m.classList.add(state);
}
function namedInputRows(su) {
  const rows = (su.inputs || []).filter((r) => r.active);
  if (su.kind) rows.unshift({ id: 'game', legacy: true, label: su.needs, role: 'game', path: su.copy, file: su.kind !== 'folder', folder: su.kind === 'folder' || su.kind === 'iso_or_folder', extensions: [] });
  if (su.update) rows.push({ id: 'legacy-update', legacy: true, label: su.update, role: 'update', optional: su.update_optional, path: su.update_copy, file: su.update_file, folder: su.update_folder, extensions: [] });
  return rows;
}
function namedDiscSummary(su) {
  const discs = namedInputRows(su).filter((r) => r.role === 'disc');
  const rows = discs.length ? discs : namedInputRows(su).filter((r) => !r.optional && !r.shared);
  return tf(discs.length ? '{ready} of {total} discs selected' : '{ready} of {total} game files selected', { ready: rows.filter((r) => r.path).length, total: rows.length });
}
function namedInputsShell(id, su, ns = {}, game = {}, tv = false) {
  const selected = namedInputRows(su).some((r) => !r.shared && r.path);
  const disc = `<div class="${tv ? 'tv-media' : 'su-media'}" aria-hidden="true">${gameMediaHtml(game, selected ? '' : 'ghost')}</div>`;
  const summary = `<p class="ni-disc-summary" role="status">${esc(namedDiscSummary(su))}</p>`;
  return tv ? `<div class="ni-tv-layout"><div class="ni-tv-art">${disc}${summary}</div><div class="ni-tv">${namedInputsView(id, su, { ...ns, tv: true }, game)}</div></div>`
    : `<div class="su-in ni-layout"><div class="su-stage ni-art">${disc}<div class="drive"><i></i></div>${summary}</div><div class="su-r ni-content">${namedInputsView(id, su, ns, game)}</div></div>`;
}
function namedInputsView(id, su, ns = {}, game = {}) {
  // DLSS (a file the launcher fetches) never holds setup back: it's always in the optional group.
  const rows = namedInputRows(su), required = rows.filter((r) => !r.optional && !r.runtime);
  const ready = required.filter((r) => r.path).length, complete = ready === required.length && !su.inputs_error;
  const error = ns.error || su.inputs_error;
  const group = (label, list) => list.length ? `<section class="ni-group" aria-label="${esc(t(label))}"><h3>${esc(t(label))}</h3><div class="ni-rows">${list.map((r) => {
    if (r.runtime) return runtimeRow(r, ns);
    const status = r.path ? r.shared ? t('Already configured') : baseName(r.path) : t('Not selected');
    const button = (act, text, extra = '') => `<button type="button" class="btn sm" data-ni="${act}" data-input="${esc(r.id)}" aria-label="${esc(t(text) + ': ' + r.label)}" ${extra} ${ns.busy ? 'disabled' : ''}>${esc(t(text))}</button>`;
    return `<div class="ni-row${r.path ? ' selected' : ''}"><span class="ni-status" aria-label="${esc(r.path ? t('Selected') : t('Not selected'))}">${r.path ? CHECK : DISC_S}</span><div class="ni-name"><b>${esc(r.label)}</b><span title="${esc(r.shared ? t('Kept on this PC for other games that need them') : r.path || '')}">${esc(status)}</span></div><div class="ni-actions">${r.file ? button('file', r.path ? 'Change' : r.folder ? 'Choose file' : 'Choose') : ''}${r.folder ? button('folder', r.path && !r.file ? 'Change' : 'Choose folder') : ''}${r.path && !r.legacy ? button(r.shared ? 'remove-key' : 'clear', 'Remove') : ''}</div></div>`;
  }).join('')}</div></section>` : '';
  const optional = rows.filter((r) => r.optional || r.runtime), added = optional.filter((r) => r.path).length;
  const optionalLabel = optional.every((r) => r.role === 'dlc') ? t('Optional DLC') : t('Optional content');
  const heading = game.game || game.name;
  const multiple = rows.filter((r) => r.role === 'disc').length > 1;
  return `<div class="ni-heading"><h2>${esc(heading ? tf(multiple ? 'Insert your copies of {game}' : 'Insert your copy of {game}', { game: heading }) : t('Choose your game files'))}</h2><p>${esc(tf('{ready} of {total} required files selected', { ready, total: required.length }))}</p></div>
    ${typeof setupLocationHtml === 'function' ? setupLocationHtml(id, su, !!ns.tv) : ''}
    ${ns.tv ? `<button type="button" class="btn sm" data-ni="open-install-folder">${esc(t('Open its folder'))}</button>` : ''}
    <div class="ni-scroll">${relinkCards(su, 'data-ni')}${group('Required files', required.filter((r) => !r.shared))}${group('Shared keys', required.filter((r) => r.shared))}${optional.length ? `<div class="ni-optional"><button type="button" class="ni-optional-toggle" data-ni="toggle-optional" aria-expanded="${!!ns.extras}" aria-controls="ni-optional-${esc(id)}"><span>${esc(optionalLabel)} <span class="ni-optional-count">${esc(tf('{added} of {total} added', { added, total: optional.length }))}</span></span><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="m6 9 6 6 6-6"/></svg></button><div id="ni-optional-${esc(id)}" ${ns.extras ? '' : 'hidden'}>${group('Optional content', optional)}</div></div>` : ''}</div>
    ${ns.confirm ? `<div class="ni-notice"><p>${esc(t('Removing shared keys affects every game that uses them. Your original file stays where it is.'))}</p><button type="button" class="btn sm" data-ni="remove-confirm" data-input="${esc(ns.confirm)}">${esc(t('Remove shared keys'))}</button><button type="button" class="btn sm" data-ni="cancel-remove">${esc(t('Keep'))}</button></div>` : ''}
    ${error ? `<p class="su-err" role="alert">${esc(t(error))}</p>` : ''}
    ${typeof prepOptsHtml === 'function' ? prepOptsHtml(su) : ''}
    <div class="ni-footer"><p>${esc(t('Optional content can be added later.'))}</p><button type="button" class="btn main" data-ni="run" ${!complete || ns.busy ? 'disabled' : ''}>${esc(t(ns.busy ? 'Working…' : 'Set up'))}</button></div>`;
}
// NVIDIA DLSS (a file the launcher fetches itself): nothing to choose. It shows where the file
// comes from, its download, the game's own file when it ships one, and a switch to turn it off.
function runtimeRow(r, ns) {
  const d = ns.dlss || {};
  const pct = d.total ? Math.floor(d.done / d.total * 100) : 0;
  const status = r.ships ? t('Using the DLSS file this game ships')
    : r.off ? t('Off for this game')
      : r.path ? t('Downloaded from NVIDIA')
        : d.failed ? t('Couldn\'t get NVIDIA\'s DLSS file. The game runs without it.')
          : d.total ? tf('Downloading from NVIDIA {pct}%', { pct })
            : t('Downloaded from NVIDIA');
  const btn = (act, text) => `<button type="button" class="btn sm" data-ni="${act}" data-input="${esc(r.id)}" aria-label="${esc(t(text) + ': ' + r.label)}" ${ns.busy ? 'disabled' : ''}>${esc(t(text))}</button>`;
  const actions = r.ships ? '' : r.off ? btn('dlss-on', 'Turn On') : `${d.failed && !r.path ? btn('dlss-retry', 'Try Again') : ''}${btn('dlss-off', 'Turn Off')}`;
  const on = !!r.path || r.ships;
  return `<div class="ni-row ni-runtime${on ? ' selected' : ''}${d.failed && !r.path && !r.off ? ' failed' : ''}"><span class="ni-status" aria-label="${esc(on ? t('Selected') : t('Not selected'))}">${on ? CHECK : DISC_S}</span><div class="ni-name"><b>${esc(r.label)}</b><span>${esc(status)}</span>${d.total && !r.path && !d.failed && !r.off ? `<i class="ni-bar" style="--p:${pct}%"></i>` : ''}</div><div class="ni-actions">${actions}</div></div>`;
}
// Fetches DLSS for a setup that wants it and hasn't got it yet: once per setup window, and again
// from Try Again. A failure never stops setup.
async function runtimeFetch(id, ns, redraw) {
  const su = S.setup[id];
  const r = su && (su.inputs || []).find((x) => x.runtime && !x.path && !x.off && !x.ships);
  if (!r || ns.dlssStarted) return;
  ns.dlssStarted = true; ns.dlss = { done: 0, total: 0 };
  const off = await T.event.listen('dlss', ({ payload: p }) => { if (p && p.total) { ns.dlss = { done: p.done, total: p.total }; redraw(); } });
  try { await invoke('dlss_fetch'); S.setup[id] = await invoke('game_setup', { id }); ns.dlss = {}; }
  catch (e) { ns.dlss = { failed: true }; }
  finally { off(); redraw(); }
}
// Use My Previous Setup (Relink): 'ready' when it's set up again, 'build' when the setup step has
// to run, or { input, label } when one file moved and only it is asked for.
async function relinkSetup(id) {
  try { await invoke('setup_relink', { id }); S.setup[id] = await invoke('game_setup', { id }); await refreshState(); return 'ready'; }
  catch (e) {
    const m = String(e && e.message ? e.message : e);
    if (m.startsWith('RELINK_BUILD|')) return 'build';
    const x = m.match(/^RELINK_MISSING\|([^|]*)\|(.*)$/);
    if (x) return { input: x[1], label: x[2] };
    throw e;
  }
}
// The two cards above the setup's own: the files it was set up with, and a build already here.
function relinkCards(su, attr) {
  if (!su) return '';
  const prev = su.previous || [];
  const card = (act, title, sub, button) => `<div class="found su-relink"><svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 12a9 9 0 0 1 15.5-6.2L21 8"/><path d="M21 3v5h-5"/><path d="M21 12a9 9 0 0 1-15.5 6.2L3 16"/><path d="M3 21v-5h5"/></svg><div class="k"><b>${esc(t(title))}</b><span title="${esc(sub)}">${esc(sub)}</span></div><button type="button" class="btn sm main" ${attr}="${act}">${esc(t(button))}</button></div>`;
  return (prev.length && !su.inputs_ready ? card('relink', 'Use My Previous Setup', prev.join(' · '), 'Use It') : '')
    + (su.prebuilt ? card('prebuilt', 'Use the Build Already on This PC', su.prebuilt, 'Use It') : '')
    + (su.own_build && !su.prebuilt ? card('ownbuild', 'Use a Build You Already Have', t('Built it before, with the game\'s own launcher or anywhere else? Pick its folder.'), 'Choose Folder') : '');
}
// A build the player already has, anywhere (Use a Build You Already Have): its folder picked, and
// used in place of building it again. True when it's set.
async function useOwnBuild(id) {
  const p = await pickFile({ title: t('The folder your build is in'), directory: true });
  if (!p) return false;
  await invoke('setup_use_build', { id, folder: typeof p === 'string' ? p : p.path });
  S.setup[id] = await invoke('game_setup', { id });
  await refreshState();
  return true;
}
async function namedInputsAction(id, act, input, ns) {
  if (ns.busy) return;
  if (act === 'open-install-folder') { await invoke('open_folder', { id: parentId(id) }).catch(fail); return; }
  if (act === 'toggle-optional') { ns.extras = !ns.extras; return; }
  if (act === 'remove-key') { ns.confirm = input; return; }
  if (act === 'cancel-remove') { ns.confirm = ''; return; }
  ns.error = ''; ns.busy = true;
  try {
    const su = S.setup[id], row = namedInputRows(su).find((r) => r.id === input);
    if (act === 'run') return 'run';
    if (act === 'relink') {
      const r = await relinkSetup(id);
      if (r === 'build') return 'run';
      if (r === 'ready') return 'ready';
      ns.error = tf('{file} isn\'t where it was. Pick it again, then press Set Up.', { file: r.label });
      return;
    }
    if (act === 'prebuilt') { await invoke('setup_use_build', { id }); S.setup[id] = await invoke('game_setup', { id }); await refreshState(); return 'ready'; }
    if (act === 'ownbuild') return (await useOwnBuild(id)) ? 'ready' : undefined;
    if (act === 'dlss-off' || act === 'dlss-on') {
      await invoke('dlss_set', { id, input, on: act === 'dlss-on' });
    } else if (act === 'dlss-retry') {
      ns.dlssStarted = false; ns.dlss = {}; ns.retry = true;
    } else if (act === 'remove-confirm') {
      await invoke('setup_key_remove', { id, input }); ns.confirm = '';
    } else if (act === 'clear') {
      await invoke('setup_input_set', { id, input, path: null });
    } else if (row && (act === 'file' || act === 'folder')) {
      const options = { title: row.label, directory: act === 'folder', multiple: false };
      if (!options.directory && row.extensions?.length) options.filters = [{ name: row.label, extensions: row.extensions }];
      const picked = await pickFile(options);
      if (!picked) return;
      const path = typeof picked === 'string' ? picked : picked.path;
      // Only the single-copy rows go to the old commands; a profile's own input may be named "game" too.
      await invoke(!row.legacy ? 'setup_input_set' : row.id === 'game' ? 'set_game_copy' : 'set_update_copy', { id, input, path });
    }
    S.setup[id] = await invoke('game_setup', { id });
    await refreshState();
  } catch (e) { ns.error = String(e && e.message ? e.message : e); }
  finally { ns.busy = false; }
}

// Desktop and Big Picture share the same rows, requirements and backend actions.
function namedSetup(id) {
  const activeJob = S.setupJobs && (S.setupJobs[id] || S.setupJobs[parentId(id)]);
  if (activeJob) return setupModal(id);
  closeSetup(true);
  Object.assign(SU, { id, stage: 'inputs', step: '', steps: {}, q: [], result: null, err: '', named: {} });
  const scrim = document.createElement('div'); scrim.className = 'scrim';
  scrim.innerHTML = `<div class="modal su ni-modal" role="dialog" aria-modal="true" aria-label="${esc(t('Set up a game'))}" tabindex="-1"><button type="button" class="su-x" data-ni="close" aria-label="${esc(t('Close'))}">${XMARK}</button><div class="ni-shell"></div></div>`;
  document.body.append(scrim); SU.el = scrim;
  const draw = () => {
    if (SU.el !== scrim) return;
    if (SU.stage !== 'inputs') return suDraw();
    const scroll = $('.ni-scroll', scrim)?.scrollTop || 0;
    const game = byId(id) || { name: S.library[id]?.name || id };
    $('.ni-shell', scrim).innerHTML = namedInputsShell(id, S.setup[id], SU.named, game);
    $('.ni-scroll', scrim).scrollTop = scroll;
    runtimeFetch(id, SU.named, draw);
  };
  scrim.addEventListener('click', async (e) => {
    const legacy = e.target.closest('[data-su]');
    if (legacy) return suAct(legacy);
    const b = e.target.closest('[data-ni]');
    if (!b || b.disabled) return;
    if (b.dataset.ni === 'close') return closeSetup();
    const act = b.dataset.ni, input = b.dataset.input;
    const pending = namedInputsAction(id, act, input, SU.named); draw();
    const result = await pending;
    if (SU.el !== scrim) return;
    if (result === 'ready') { SU.stage = 'ready'; SU.result = { text: t('Your previous setup') }; $('.modal', scrim).classList.remove('ni-modal'); return suDraw(); }
    if (result === 'run') {
      SU.stage = 'run'; SU.step = 'read'; SU.steps = {}; SU.q = []; SU.at = Date.now(); SU.path = ''; $('.modal', scrim).classList.remove('ni-modal'); suDraw();
      try { await invoke('setup_inputs_run', { id }); }
      catch (e) { if (SU.el === scrim) { SU.stage = 'error'; SU.err = String(e?.message || e); suDraw(); } }
    } else {
      draw();
      const restore = $$('[data-ni]', scrim).find((x) => x.dataset.ni === act && x.dataset.input === input && !x.disabled);
      (restore || $('.ni-content button:not([disabled])', scrim) || $('.modal', scrim)).focus({ preventScroll: true });
    }
  });
  scrim.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') { e.preventDefault(); closeSetup(); return; }
    if (e.key !== 'Tab') return;
    const buttons = $$('button:not([disabled])', scrim).filter((b) => !b.closest('[hidden]')), first = buttons[0], last = buttons[buttons.length - 1];
    if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last?.focus(); }
    else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first?.focus(); }
  });
  draw(); ($('.ni-content button:not([disabled])', scrim) || $('.modal', scrim)).focus();
}
