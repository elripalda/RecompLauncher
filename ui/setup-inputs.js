// Named Easy Setup inputs. Shared reuse is automatic; requirements come only from the profile.
// Kept separate from the single-copy flow so existing profiles retain their setup behavior.
function namedInputRows(su) {
  const rows = (su.inputs || []).filter((r) => r.active);
  if (su.kind) rows.unshift({ id: 'game', label: su.needs, role: 'game', path: su.copy, file: su.kind !== 'folder', folder: su.kind === 'folder' || su.kind === 'iso_or_folder', extensions: [] });
  if (su.update) rows.push({ id: 'legacy-update', label: su.update, role: 'update', optional: su.update_optional, path: su.update_copy, file: su.update_file, folder: su.update_folder, extensions: [] });
  return rows;
}
function namedDiscSummary(su) {
  const discs = namedInputRows(su).filter((r) => r.role === 'disc');
  const rows = discs.length ? discs : namedInputRows(su).filter((r) => !r.optional && !r.shared);
  return tf(discs.length ? '{ready} of {total} discs selected' : '{ready} of {total} game files selected', { ready: rows.filter((r) => r.path).length, total: rows.length });
}
function namedInputsShell(id, su, ns = {}, game = {}, tv = false) {
  const selected = namedInputRows(su).some((r) => !r.shared && r.path);
  const artwork = game.cover ? pic(game.cover) : '';
  const disc = `<div class="${tv ? 'bigdisc' : 'sdisc'}${selected ? '' : ' ghost'}" aria-hidden="true"><div class="lab">${artwork}</div><div class="sheen"></div><div class="hub"></div><div class="hole"></div></div>`;
  const summary = `<p class="ni-disc-summary" role="status">${esc(namedDiscSummary(su))}</p>`;
  return tv ? `<div class="ni-tv-layout"><div class="ni-tv-art">${disc}${summary}</div><div class="ni-tv">${namedInputsView(id, su, ns, game)}</div></div>`
    : `<div class="su-in ni-layout"><div class="su-stage ni-art">${disc}<div class="drive"><i></i></div>${summary}</div><div class="su-r ni-content">${namedInputsView(id, su, ns, game)}</div></div>`;
}
function namedInputsView(id, su, ns = {}, game = {}) {
  const rows = namedInputRows(su), required = rows.filter((r) => !r.optional);
  const ready = required.filter((r) => r.path).length, complete = ready === required.length && !su.inputs_error;
  const error = ns.error || su.inputs_error;
  const group = (label, list) => list.length ? `<section class="ni-group" aria-label="${esc(t(label))}"><h3>${esc(t(label))}</h3><div class="ni-rows">${list.map((r) => {
    const status = r.path ? r.shared ? t('Already configured') : baseName(r.path) : t('Not selected');
    const button = (act, text, extra = '') => `<button type="button" class="btn sm" data-ni="${act}" data-input="${esc(r.id)}" aria-label="${esc(t(text) + ': ' + r.label)}" ${extra} ${ns.busy ? 'disabled' : ''}>${esc(t(text))}</button>`;
    return `<div class="ni-row${r.path ? ' selected' : ''}"><span class="ni-status" aria-label="${esc(r.path ? t('Selected') : t('Not selected'))}">${r.path ? CHECK : DISC_S}</span><div class="ni-name"><b>${esc(r.label)}</b><span title="${esc(r.shared ? t('Kept on this PC for other games that need them') : r.path || '')}">${esc(status)}</span></div><div class="ni-actions">${r.file ? button('file', r.path ? 'Change' : r.folder ? 'Choose file' : 'Choose') : ''}${r.folder ? button('folder', r.path && !r.file ? 'Change' : 'Choose folder') : ''}${r.path && r.id !== 'game' && r.id !== 'legacy-update' ? button(r.shared ? 'remove-key' : 'clear', 'Remove') : ''}</div></div>`;
  }).join('')}</div></section>` : '';
  const optional = rows.filter((r) => r.optional), added = optional.filter((r) => r.path).length;
  const optionalLabel = optional.every((r) => r.role === 'dlc') ? t('Optional DLC') : t('Optional content');
  const heading = game.game || game.name;
  const multiple = rows.filter((r) => r.role === 'disc').length > 1;
  return `<div class="ni-heading"><h2>${esc(heading ? tf(multiple ? 'Insert your copies of {game}' : 'Insert your copy of {game}', { game: heading }) : t('Choose your game files'))}</h2><p>${esc(tf('{ready} of {total} required files selected', { ready, total: required.length }))}</p></div>
    <div class="ni-scroll">${group('Required files', required.filter((r) => !r.shared))}${group('Shared keys', required.filter((r) => r.shared))}${optional.length ? `<div class="ni-optional"><button type="button" class="ni-optional-toggle" data-ni="toggle-optional" aria-expanded="${!!ns.extras}" aria-controls="ni-optional-${esc(id)}"><span>${esc(optionalLabel)} <span class="ni-optional-count">${esc(tf('{added} of {total} added', { added, total: optional.length }))}</span></span><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="m6 9 6 6 6-6"/></svg></button><div id="ni-optional-${esc(id)}" ${ns.extras ? '' : 'hidden'}>${group('Optional content', optional)}</div></div>` : ''}</div>
    ${ns.confirm ? `<div class="ni-notice"><p>${esc(t('Removing shared keys affects every game that uses them. Your original file stays where it is.'))}</p><button type="button" class="btn sm" data-ni="remove-confirm" data-input="${esc(ns.confirm)}">${esc(t('Remove shared keys'))}</button><button type="button" class="btn sm" data-ni="cancel-remove">${esc(t('Keep'))}</button></div>` : ''}
    ${error ? `<p class="su-err" role="alert">${esc(t(error))}</p>` : ''}
    <div class="ni-footer"><p>${esc(t('Optional content can be added later.'))}</p><button type="button" class="btn main" data-ni="run" ${!complete || ns.busy ? 'disabled' : ''}>${esc(t(ns.busy ? 'Working…' : 'Set up'))}</button></div>`;
}
async function namedInputsAction(id, act, input, ns) {
  if (ns.busy) return;
  if (act === 'toggle-optional') { ns.extras = !ns.extras; return; }
  if (act === 'remove-key') { ns.confirm = input; return; }
  if (act === 'cancel-remove') { ns.confirm = ''; return; }
  ns.error = ''; ns.busy = true;
  try {
    const su = S.setup[id], row = namedInputRows(su).find((r) => r.id === input);
    if (act === 'run') return 'run';
    if (act === 'remove-confirm') {
      await invoke('setup_key_remove', { id, input }); ns.confirm = '';
    } else if (act === 'clear') {
      await invoke('setup_input_set', { id, input, path: null });
    } else if (row && (act === 'file' || act === 'folder')) {
      const options = { title: row.label, directory: act === 'folder', multiple: false };
      if (!options.directory && row.extensions?.length) options.filters = [{ name: row.label, extensions: row.extensions }];
      const picked = await pickFile(options);
      if (!picked) return;
      const path = typeof picked === 'string' ? picked : picked.path;
      await invoke(row.id === 'game' ? 'set_game_copy' : row.id === 'legacy-update' ? 'set_update_copy' : 'setup_input_set', { id, input, path });
    }
    S.setup[id] = await invoke('game_setup', { id });
    await refreshState();
  } catch (e) { ns.error = String(e && e.message ? e.message : e); }
  finally { ns.busy = false; }
}

// Desktop and Big Picture share the same rows, requirements and backend actions.
function namedSetup(id) {
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
