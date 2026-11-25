// Easy Setup in steps: Game (Discs when there are several), then Add-ons, then Extras. A step with
// nothing in it isn't there, so a game that needs one file is one page. One screen for every
// profile (game and game.update, or named inputs), desktop and Big Picture.
//
// Everything the game needs is a box or a row with its buttons on it. Something the launcher found
// or remembered is shown in its own box, with a tag saying where it came from, never in a card of
// its own. The boxes keep their size and their buttons in every state, so nothing moves.
//
// ns is the window's own state: the step it's on (at), the copy picked here (copy: a game.kind
// copy is only kept by setup_copy when Set Up starts), and the rows' download and error
// state shared with setup-inputs.js.

const SS_FOLDER = '<svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" aria-hidden="true"><path d="M3 6.5A1.5 1.5 0 0 1 4.5 5H10l2 2.5h7.5A1.5 1.5 0 0 1 21 9v9.5a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 18.5z"/></svg>';
const SS_WARN = '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" aria-hidden="true"><circle cx="12" cy="12" r="9"/><path d="M12 7.5v5.5M12 16.5v.5"/></svg>';

// A game on several discs: the copy box holds the first one (the box always says role `game`), so
// it counts as a disc with the rest. A game with no disc rows has none.
function discRows(own) {
  const discs = own.filter((r) => r.role === 'disc');
  const copy = own.find((r) => r.legacy && r.id === 'game');
  return discs.length && copy ? [copy, ...discs] : discs;
}

// The rows, with where each one's file came from (how): 'found' (the folder scan), 'prev' (kept
// from before this window opened), 'shared' (keys kept on this PC), or '' (picked just now).
function stepsRows(id, su, ns) {
  const rows = namedInputRows(su).map((r) => Object.assign({}, r));
  const picked = ns.picked || {};
  for (const r of rows) r.how = r.path ? (r.shared ? 'shared' : picked[r.id] ? '' : 'prev') : '';
  const game = rows.find((r) => r.legacy && r.id === 'game');
  if (!game) return rows;
  // A copy that is only a folder takes no file types, whatever files its console has.
  const fm = copyFormats(id);
  game.extensions = game.file ? fm.exts : [];
  if (ns.copy) Object.assign(game, { path: ns.copy, how: '' });
  else if (!game.path && S.owned[id]) Object.assign(game, { path: S.owned[id].path, how: 'found' });
  return rows;
}

// What the window shows and what Set Up waits for, worked out once per draw.
function stepsModel(id, su, ns) {
  const rows = stepsRows(id, su, ns);
  const needed = rows.filter((r) => !r.optional && !r.runtime);
  const own = needed.filter((r) => !r.shared);
  const keys = needed.filter((r) => r.shared);
  const addons = rows.filter((r) => r.optional && !r.runtime);
  const runtime = rows.filter((r) => r.runtime);
  const opts = su.prepare_options || [];
  // Its setup step runs again without the copy, from what it kept (prepare.keeps_copy).
  const kept = !!su.kept;
  const missing = needed.filter((r) => !r.path && !(kept && r.legacy && r.id === 'game'));
  const reqOff = prepBlocked(su);
  // Options its project added since the player set it up: the Extras step has a dot.
  const fresh = opts.filter((o) => o.new && (su.copy || su.inputs_ready));
  const steps = [{ id: 'game', name: discRows(own).length > 1 ? t('Discs') : t('Game') }];
  if (addons.length) steps.push({ id: 'addons', name: t('Add-ons') });
  if (opts.length || runtime.length) steps.push({ id: 'extras', name: t('Extras') });
  const at = Math.max(0, Math.min(ns.at || 0, steps.length - 1));
  const last = at === steps.length - 1;
  const broken = !!su.inputs_error;
  const blocked = missing.length > 0 || reqOff || broken || !!ns.busy;
  return { rows, own, keys, addons, runtime, opts, fresh, kept, missing, reqOff, steps, at, cur: steps[at], last, broken, blocked };
}

const ssTag = (r) => {
  if (r.how === 'found') return `<span class="ss-src">${esc(t('Found in your folders'))}</span>`;
  if (r.how === 'prev') return `<span class="ss-src prev">${esc(t('Used last time'))}</span>`;
  if (r.how === 'shared') return `<span class="ss-src shared">${esc(t('Already on this PC'))}</span>`;
  return '';
};
const ssFmts = (r) => (r.extensions || []).map((e) => `<span class="fmt">.${esc(e)}</span>`).join('');
// An input the manifest lets be only a folder: its picker shows no files, so a player holding a
// file (an .nsp dump) would see nothing happen. Said from its declared formats alone, never the
// file's extension or the console; a recomp that wants more (exefs and romfs) says it in its label.
const ssFolderOnly = (r) => !!r.folder && !r.file;
// "This game" only on the game itself (its copy or a disc); an update or add-on folder is "This".
const ssIsGame = (r) => (r.legacy && r.id === 'game') || r.role === 'game' || r.role === 'disc';
function ssFolderHint(r) {
  const text = ssIsGame(r) ? t('This game takes a folder, not a file.') : t('This takes a folder, not a file.');
  return `<span class="ss-hint">${esc(text)}</span>`;
}
const ssEmptyMeta = (r) => (ssFolderOnly(r) ? ssFolderHint(r) : `<span class="fmts">${ssFmts(r)}</span>`);
// The ways to give a row its file. The same buttons empty or filled: choosing again changes it.
function ssPickBtns(r, busy) {
  const off = busy ? ' disabled' : '';
  const file = r.file ? `<button type="button" class="btn sm" data-ss="file" data-input="${esc(r.id)}" aria-label="${esc(t('Choose File') + ': ' + r.label)}"${off}>${esc(r.folder ? t('Choose File') : t('Choose'))}</button>` : '';
  const folder = r.folder ? `<button type="button" class="btn sm" data-ss="folder" data-input="${esc(r.id)}" aria-label="${esc(t('Choose Folder') + ': ' + r.label)}"${off}>${esc(t('Choose Folder'))}</button>` : '';
  return file + folder;
}

// The one big box, for a game that needs one thing: three lines whatever its state (what it is,
// the file or what to choose, and where it came from or the formats it takes).
function ssBox(id, su, ns, r, title) {
  const on = !!r.path;
  const line = on ? baseName(r.path) : r.label;
  const meta = on ? ssTag(r) : ssEmptyMeta(r);
  const icon = on ? CHECK : r.file ? DISC_L : SS_FOLDER;
  return `<div class="ss-box${on ? ' on' : ''}"><span class="ss-ic">${icon}</span><div class="k"><b>${esc(title)}</b><span title="${esc(on ? r.path : r.label)}">${esc(line)}</span></div><div class="low"><div class="meta">${meta}</div><div class="acts">${ssPickBtns(r, ns.busy)}</div></div></div>`;
}

// A compact row: discs, keys, updates, add-ons. Remove keeps its place while the row is empty.
function ssRow(r, ns) {
  const on = !!r.path;
  const sub = on ? (r.shared ? t('Kept once on this PC for every game that needs it') : baseName(r.path)) : r.legacy || ssFolderOnly(r) ? '' : ssFmts(r) ? '' : t('Not chosen yet');
  const fmts = on ? '' : ssFolderOnly(r) ? ssFolderHint(r) : ssFmts(r) ? `<span class="fmts">${ssFmts(r)}</span>` : '';
  const canRemove = r.optional || (r.shared && !r.legacy);
  const rm = canRemove ? `<button type="button" class="btn sm rm" data-ss="${r.shared ? 'remove-key' : 'clear'}" data-input="${esc(r.id)}" aria-label="${esc(t('Remove') + ': ' + r.label)}" title="${esc(t('Remove'))}"${on && !ns.busy ? '' : ' disabled'}>${XMARK}</button>` : '';
  // A shared key's path is the launcher's private copy: never shown, not even as a tooltip.
  const tip = r.shared ? '' : ` title="${esc(r.path || '')}"`;
  return `<div class="ss-row${on ? ' on' : ''}${r.optional ? ' opt' : ''}"><span class="st">${CHECK}</span><div class="k"><b>${esc(r.label)}</b><div class="line">${sub ? `<span${tip}>${esc(sub)}</span>` : ''}${fmts}${on ? ssTag(r) : ''}</div></div><div class="acts">${ssPickBtns(r, ns.busy)}${rm}</div></div>`;
}

// Extras: the setup step's options (switches; one with a license starts off and the player turns
// it on) and NVIDIA DLSS, which the launcher fetches itself.
function ssOption(o) {
  const req = o.required ? ` <span class="su-req-tag">${esc(t('Required'))}</span>` : '';
  const fresh = o.new ? ` <span class="su-new">${esc(t('New'))}</span>` : '';
  const lic = o.license && o.license.url ? htmlFill(t('Turning it on accepts the {license}.'), { license: `<a href="#" data-url="${esc(o.license.url)}">${esc(o.license.name || o.license.url)}</a>` }) : '';
  const small = [o.note ? esc(o.note) : '', lic].filter(Boolean).join(' ');
  return `<label class="su-opt"><span class="su-opt-text"><b>${esc(o.label)}</b>${req}${fresh}${small ? `<small>${small}</small>` : ''}</span><input type="checkbox" role="switch" data-popt="${esc(o.id)}"${o.on ? ' checked' : ''}${o.required ? ' aria-required="true"' : ''}><span class="switch" aria-hidden="true"></span></label>`;
}
function ssRuntime(r, ns) {
  const d = ns.dlss || {};
  const pct = d.total ? Math.floor((d.done / d.total) * 100) : 0;
  if (r.ships) return `<div class="su-opt ss-plain"><span class="su-opt-text"><b>${esc(r.label)}</b><small>${esc(t('Using the DLSS file this game ships'))}</small></span></div>`;
  const status = r.off ? t('Off for this game')
    : r.path ? t('Downloaded from NVIDIA')
      : d.failed ? t('Couldn\'t get NVIDIA\'s DLSS file. The game runs without it.')
        : d.total ? tf('Downloading from NVIDIA {pct}%', { pct })
          : t('Downloaded from NVIDIA');
  const bar = d.total && !r.path && !d.failed && !r.off ? `<i class="ni-bar" style="--p:${pct}%"></i>` : '';
  const retry = d.failed && !r.path && !r.off ? `<button type="button" class="btn sm" data-ss="dlss-retry" data-input="${esc(r.id)}">${esc(t('Try Again'))}</button>` : '';
  const sw = `<button type="button" class="switch" role="switch" aria-checked="${!r.off}" aria-label="${esc(r.label)}" data-ss="${r.off ? 'dlss-on' : 'dlss-off'}" data-input="${esc(r.id)}"${ns.busy ? ' disabled' : ''}></button>`;
  return `<div class="su-opt ss-plain${d.failed && !r.path && !r.off ? ' failed' : ''}"><span class="su-opt-text"><b>${esc(r.label)}</b><small>${esc(status)}</small>${bar}</span>${retry}${sw}</div>`;
}

// The right side of the window.
function stepsView(id, su, ns, game) {
  const m = stepsModel(id, su, ns);
  const name = game.name || id;
  // On Extras with new options: they count as seen (Setup Options' dots go), once per window.
  if (m.cur.id === 'extras' && m.fresh.length && !ns.seenNew && typeof seenNewOpts === 'function') {
    ns.seenNew = true;
    seenNewOpts(id, m.fresh);
  }
  const rail = m.steps.length > 1 ? `<div class="ss-rail">${m.steps.map((s, i) => `${i ? '<span class="bar"></span>' : ''}<span class="s${i === m.at ? ' now' : i < m.at ? ' done' : ''}${s.id === 'extras' && m.reqOff ? ' req' : ''}${s.id === 'extras' && m.fresh.length ? ' new' : ''}"><i>${i < m.at ? CHECK : i + 1}</i>${esc(s.name)}</span>`).join('')}</div>` : '';
  let head = '', lead = '', body = '', status = '';
  if (m.cur.id === 'game') {
    head = discRows(m.own).length > 1 ? t('Insert your discs') : t('Insert your copy');
    lead = t('Checked once when it\'s set up. After that, you just press Play.');
    // Built again from what its setup step kept: the copy's box stays, for building from it instead.
    const copyRow = m.own.find((r) => r.legacy && r.id === 'game');
    if (m.kept && copyRow && !copyRow.path) {
      head = t('Build It Again');
      lead = t('Your copy isn\'t needed: it\'s built again from what it kept last time.');
    }
    const one = m.own.length === 1 && (m.own[0].legacy || m.own[0].role === 'game' || m.own[0].role === 'disc');
    // The box names the game on top and says what to choose under it, so an input's label (often
    // long, with the file name in it) is said once, not twice.
    const title = game.game || name;
    const main = one ? ssBox(id, su, ns, m.own[0], title) : m.own.length ? `<div class="ss-rows">${m.own.map((r) => ssRow(r, ns)).join('')}</div>` : '';
    const keys = m.keys.length ? `<div><p class="ss-cap">${esc(t('Keys'))} <i>${esc(t('kept once on this PC, for every game that needs them'))}</i></p><div class="ss-rows">${m.keys.map((r) => ssRow(r, ns)).join('')}</div></div>` : '';
    body = main + keys;
    status = m.missing.length ? tn(m.missing.length, 'Choose 1 more file to go on.', 'Choose {n} more files to go on.') : t('Everything it needs is here.');
  } else if (m.cur.id === 'addons') {
    head = t('Add-ons');
    lead = t('Updates and DLC you have. All optional, and you can add them later.');
    body = `<div class="ss-rows">${m.addons.map((r) => ssRow(r, ns)).join('')}</div>`;
    status = tf('{added} of {total} added.', { added: m.addons.filter((r) => r.path).length, total: m.addons.length });
  } else {
    head = t('Extras');
    const reqN = m.opts.filter((o) => o.required).length;
    lead = t('Optional. You can change them later in Setup Options.');
    if (m.fresh.length) lead = tf('New since you set it up: {names}.', { names: m.fresh.map((o) => o.label).join(', ') });
    if (reqN) lead = reqN < m.opts.length + m.runtime.length ? t('Turn on the ones marked Required to set it up. The rest are optional.') : reqN === 1 ? t('Setup needs this one turned on.') : t('Setup needs these turned on.');
    body = `<div class="ss-opts su-opts">${m.opts.map((o) => ssOption(o)).join('')}${m.runtime.map((r) => ssRuntime(r, ns)).join('')}</div>`;
  }
  if (m.last) {
    body += ssSummary(su, ns, m);
    if (m.cur.id !== 'game') status = m.missing.length ? tn(m.missing.length, '1 file missing on step 1.', '{n} files missing on step 1.') : m.reqOff ? prepReq() : t('Ready to set up.');
    else if (!m.missing.length) status = m.reqOff ? prepReq() : t('Ready to set up.');
  }
  // Rather set it up yourself: Easy Setup off for this game, from its first step. Its Manage tab
  // has the switch to turn it back on.
  const skippable = m.at === 0 && S.easyAll && S.easyAll.has(id) && !(S.settings.ui || {}).easy_skip_all;
  if (skippable) body += `<p class="ss-alt">${esc(t('Rather set it up yourself?'))} <button type="button" data-ss="skip-easy">${esc(t('Skip Easy Setup'))}</button></p>`;
  // The check mark only on the lines that say it's ready.
  const ok = [t('Everything it needs is here.'), t('Ready to set up.')].includes(status);
  const error = ns.error || su.inputs_error;
  const notice = ns.confirm ? `<div class="ni-notice"><p>${esc(t('Removing shared keys affects every game that uses them. Your original file stays where it is.'))}</p><button type="button" class="btn sm" data-ss="remove-confirm" data-input="${esc(ns.confirm)}">${esc(t('Remove shared keys'))}</button> <button type="button" class="btn sm" data-ss="cancel-remove">${esc(t('Keep'))}</button></div>` : '';
  // Back keeps its place on the first step too (hidden), so the footer doesn't move between steps.
  const back = m.steps.length > 1 ? `<button type="button" class="btn big${m.at ? '' : ' ghost'}" data-ss="back"${m.at ? '' : ' tabindex="-1" aria-hidden="true" disabled'}>${esc(t('Back'))}</button>` : '';
  const next = m.last ? `<button type="button" class="btn main big" data-ss="run"${m.blocked ? ' disabled' : ''}>${esc(ns.busy ? t('Working…') : t('Set Up'))}</button>`
    : `<button type="button" class="btn main big" data-ss="next"${m.cur.id === 'game' && (m.missing.length || m.broken) ? ' disabled' : ''}>${esc(t('Next'))}</button>`;
  return `<div class="ss-head"><span class="kick">${esc(t('Easy Setup'))} · ${esc(name)}</span>${rail}<h2>${esc(head)}</h2><p>${esc(lead)}</p></div>
    <div class="ss-scroll">${body}${notice}${error ? `<p class="su-err" role="alert">${esc(t(error))}</p>` : ''}</div>
    <div class="ss-foot">${back}<p class="why" role="status">${ok ? CHECK : ''}${esc(status)}</p>${next}</div>`;
}

// The last step sums up, and says what Set Up starts: the developer's own prepare.label.
function ssSummary(su, ns, m) {
  const files = tf('{ready} of {total} ready', { ready: m.own.concat(m.keys).filter((r) => r.path).length, total: m.own.length + m.keys.length });
  const addons = m.addons.length ? `<div><span>${esc(t('Add-ons'))}</span><b>${esc(tf('{added} of {total} added', { added: m.addons.filter((r) => r.path).length, total: m.addons.length }))}</b></div>` : '';
  const then = su.prepare_label || '';
  // A setup that builds outside the game's own folder says where (build_where), as it always has.
  const where = su.build_where ? `<div><span>${esc(t('Builds into'))}</span><b>${esc(su.build_where)}</b></div>` : '';
  // On a one-page setup the box above already shows the files: only what Set Up starts.
  const count = m.steps.length > 1 ? `<div><span>${esc(t('Game files'))}</span><b>${esc(files)}</b></div>${addons}` : '';
  const rows = `${count}${then ? `<div><span>${esc(t('Then'))}</span><b>${esc(then)}</b></div>` : ''}${where}`;
  return rows ? `<div class="ss-sum">${rows}</div>` : '';
}

// The left side: the game's media, ghosted (with the drop hint on the desktop) until a file is in.
function stepsShell(id, su, ns, game, tv) {
  const m = stepsModel(id, su, ns);
  const any = m.own.some((r) => r.path);
  const discs = discRows(m.own);
  const sum = discs.length > 1 ? `<p class="ni-disc-summary" role="status">${esc(tf('{ready} of {total} discs selected', { ready: discs.filter((r) => r.path).length, total: discs.length }))}</p>` : '';
  const art = gameMediaHtml(game, any ? '' : 'ghost');
  // The drop area asks for what a drop fills (stepsDrop: the copy, else the first empty row, which
  // is the first row while nothing is in): a folder when that's all that row takes.
  const dropRow = m.own.find((r) => r.legacy && r.id === 'game') || m.own[0];
  const folderDrop = !!dropRow && ssFolderOnly(dropRow);
  const dropText = folderDrop ? t('Drop your game\'s folder here') : copyDrop(copyFormats(id));
  if (tv) return `<div class="ni-tv-layout"><div class="ni-tv-art"><div class="tv-media" aria-hidden="true">${art}</div>${sum}</div><div class="ni-tv su-r ss">${stepsView(id, su, ns, game)}</div></div>`;
  const drop = any ? `<div class="su-media">${art}</div>` : `<div class="su-drop"><div class="su-media">${art}</div><b>${esc(dropText)}</b><span>${esc(t('or choose it on the right'))}</span></div>`;
  return `<div class="su-in ss-in"><div class="su-stage">${drop}<div class="drive" aria-hidden="true"><i></i></div>${sum}</div><div class="su-r ss">${stepsView(id, su, ns, game)}</div></div>`;
}

// What a button does. Most keep the window as it is and redraw; 'run' is the host's to start.
async function stepsAction(id, act, input, ns) {
  const su = S.setup[id];
  const row = input ? stepsRows(id, su, ns).find((r) => r.id === input) : null;
  if (act === 'next') { ns.at = (ns.at || 0) + 1; return; }
  if (act === 'back') { ns.at = Math.max(0, (ns.at || 0) - 1); return; }
  if (act === 'run') return 'run';
  if (act === 'skip-easy') return 'skip';
  // The game's copy (game.kind) stays on this screen until Set Up: setup_copy checks it and keeps
  // it then, so a window closed halfway leaves the game as it was.
  if (row && row.legacy && row.id === 'game' && (act === 'file' || act === 'folder')) {
    const p = await pickFile(act === 'folder' ? { title: t('The game\'s folder'), directory: true } : copyPickOpts(id)).catch(fail);
    if (p) ns.copy = typeof p === 'string' ? p : p.path;
    return;
  }
  const verb = { file: 'file', folder: 'folder', clear: 'clear', 'remove-key': 'remove-key', 'remove-confirm': 'remove-confirm', 'cancel-remove': 'cancel-remove', 'dlss-on': 'dlss-on', 'dlss-off': 'dlss-off', 'dlss-retry': 'dlss-retry' }[act];
  if (!verb) return;
  const before = row && row.path;
  await namedInputsAction(id, verb, input, ns);
  const after = stepsRows(id, S.setup[id], ns).find((r) => r.id === input);
  if ((act === 'file' || act === 'folder') && after && after.path && after.path !== before) ns.picked = Object.assign({}, ns.picked, { [input]: true });
}

// A file dropped on the window goes into the next box that's empty: the copy first.
async function stepsDrop(id, path, ns) {
  const su = S.setup[id];
  const m = stepsModel(id, su, ns);
  const game = m.own.find((r) => r.legacy && r.id === 'game');
  if (game) { ns.copy = path; return; }
  const empty = m.own.find((r) => !r.path) || m.addons.find((r) => !r.path);
  if (!empty) return;
  try {
    await invoke('setup_input_set', { id, input: empty.id, path, pending: true });
    S.setup[id] = await invoke('game_setup', { id });
    ns.picked = Object.assign({}, ns.picked, { [empty.id]: true });
  } catch (e) { ns.error = String(e && e.message ? e.message : e); }
}

// Set Up: what starts, by what the screen has. { ready: text } when nothing has to run,
// { copy: path } for the copy in its box (setup_copy, or setup_inputs_run with it when the game
// has named inputs too), { kept: true } for setup_copy without the copy (built again from
// what it kept), { inputs: true } for setup_inputs_run, or nothing when it
// couldn't (the reason is in ns). saving: the options' save still on its way, which the backend
// checks the required ones against.
async function stepsRun(id, ns, saving) {
  const su = S.setup[id];
  const m = stepsModel(id, su, ns);
  if (m.blocked) return null;
  ns.error = '';
  await saving;
  // The copy first, named inputs or not: it's only on this screen until Set Up (#374).
  const game = m.own.find((r) => r.legacy && r.id === 'game');
  if (game && game.path) return { copy: game.path };
  if ((su.inputs || []).length) return { inputs: true };
  return m.kept ? { kept: true } : null;
}
