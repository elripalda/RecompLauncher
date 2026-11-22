// Named Easy Setup inputs: the rows and what their buttons do. The steps (setup-steps.js) draw
// them, desktop and Big Picture. Shared reuse is automatic; requirements come only from the profile.

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
  // The game's main file (the copy): the big box. Its update, discs and keys are the rows after it.
  if (su.kind) rows.unshift({ id: 'game', legacy: true, label: su.needs, role: 'game', path: su.copy, file: su.kind !== 'folder', folder: su.kind === 'folder' || su.kind === 'iso_or_folder', extensions: su.copy_extensions || [] });
  return rows;
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
async function namedInputsAction(id, act, input, ns) {
  if (ns.busy) return;
  if (act === 'toggle-optional') { ns.extras = !ns.extras; return; }
  if (act === 'remove-key') { ns.confirm = input; return; }
  if (act === 'cancel-remove') { ns.confirm = ''; return; }
  ns.error = ''; ns.busy = true;
  try {
    const su = S.setup[id], row = namedInputRows(su).find((r) => r.id === input);
    if (act === 'run') return 'run';
    if (act === 'dlss-off' || act === 'dlss-on') {
      await invoke('dlss_set', { id, input, on: act === 'dlss-on' });
    } else if (act === 'dlss-retry') {
      ns.dlssStarted = false; ns.dlss = {}; ns.retry = true;
    } else if (act === 'remove-confirm') {
      await invoke('setup_key_remove', { id, input }); ns.confirm = '';
    } else if (act === 'clear') {
      await invoke('setup_input_set', { id, input, path: null, pending: true });
    } else if (row && (act === 'file' || act === 'folder')) {
      const options = { title: row.label, directory: act === 'folder', multiple: false };
      if (!options.directory && row.extensions?.length) options.filters = [{ name: row.label, extensions: row.extensions }];
      const picked = await pickFile(options);
      if (!picked) return;
      const path = typeof picked === 'string' ? picked : picked.path;
      // Picked in Easy Setup's window: it waits there until a setup with it works.
      await invoke(row.legacy ? 'set_game_copy' : 'setup_input_set', { id, input, path, pending: true });
    }
    S.setup[id] = await invoke('game_setup', { id });
    await refreshState();
  } catch (e) { ns.error = String(e && e.message ? e.message : e); }
  finally { ns.busy = false; }
}
