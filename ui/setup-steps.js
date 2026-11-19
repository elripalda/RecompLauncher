// Easy Setup in steps: Game (Discs when there are several), then Add-ons, then Extras. A step with
// nothing in it isn't there, so a game that needs one file is one page. One screen for every
// profile (game and game.update, or named inputs), desktop and Big Picture.
//
// Everything the game needs is a box or a row with its buttons on it. Something the launcher found
// or remembered is shown in its own box, with a tag saying where it came from, never in a card of
// its own. The boxes keep their size and their buttons in every state, so nothing moves.
//
// ns is the window's own state: the step it's on (at), the copy picked here (copy: a game.kind
// copy is only kept by setup_copy when Set Up starts), the player's own build (own), and the
// rows' download and error state shared with setup-inputs.js.

const SS_FOLDER = '<svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" aria-hidden="true"><path d="M3 6.5A1.5 1.5 0 0 1 4.5 5H10l2 2.5h7.5A1.5 1.5 0 0 1 21 9v9.5a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 18.5z"/></svg>';
const SS_WARN = '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" aria-hidden="true"><circle cx="12" cy="12" r="9"/><path d="M12 7.5v5.5M12 16.5v.5"/></svg>';

// The rows, with where each one's file came from (how): 'found' (the folder scan), 'prev' (kept
// from before this window opened), 'shared' (keys kept on this PC), or '' (picked just now).
function stepsRows(id, su, ns) {
  const rows = namedInputRows(su).map((r) => Object.assign({}, r));
  const picked = ns.picked || {};
  for (const r of rows) r.how = r.path ? (r.shared ? 'shared' : picked[r.id] ? '' : 'prev') : '';
  const game = rows.find((r) => r.legacy && r.id === 'game');
  if (!game) return rows;
  const fm = copyFormats(id);
  game.extensions = fm.exts;
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
  // A build that's already there stands in for the copy: the player's own (Choose That Folder), or
  // the one in the game's folder from before, unless they asked to build it again.
  const skipBuild = !!ns.own || (!!su.prebuilt && !ns.rebuild);
  // Its setup step runs again without the copy, from what it kept (prepare.keeps_copy).
  const kept = !!su.kept && !skipBuild;
  const missing = needed.filter((r) => !r.path && !((skipBuild || kept) && r.legacy && r.id === 'game'));
  // A build option, required or not, does nothing when nothing is built.
  const reqOff = !skipBuild && prepBlocked(su);
  // Options its project added since the player set it up: the Extras step has a dot.
  const fresh = opts.filter((o) => o.new && (su.copy || su.inputs_ready));
  const steps = [{ id: 'game', name: own.filter((r) => r.role === 'disc').length > 1 ? t('Discs') : t('Game') }];
  if (addons.length) steps.push({ id: 'addons', name: t('Add-ons') });
  if (opts.length || runtime.length) steps.push({ id: 'extras', name: t('Extras') });
  const at = Math.max(0, Math.min(ns.at || 0, steps.length - 1));
  const last = at === steps.length - 1;
  const broken = !!su.inputs_error;
  // The player's own build didn't pass (ownErr): Set Up waits for another folder or their copy.
  const ownBad = !!(ns.own && ns.ownErr);
  const blocked = missing.length > 0 || reqOff || broken || ownBad || !!ns.busy;
  return { rows, own, keys, addons, runtime, opts, fresh, skipBuild, kept, missing, reqOff, ownBad, steps, at, cur: steps[at], last, broken, blocked };
}

const ssTag = (r) => {
  if (r.how === 'found') return `<span class="ss-src">${esc(t('Found in your folders'))}</span>`;
  if (r.how === 'prev') return `<span class="ss-src prev">${esc(t('Used last time'))}</span>`;
  if (r.how === 'shared') return `<span class="ss-src shared">${esc(t('Already on this PC'))}</span>`;
  return '';
};
const ssFmts = (r) => (r.extensions || []).map((e) => `<span class="fmt">.${esc(e)}</span>`).join('');
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
  if (ns.own) {
    const bad = ns.ownErr;
    const line = bad || baseName(ns.own);
    const tag = bad ? `<span class="ss-src warn">${esc(t('Not a finished build'))}</span>` : `<span class="ss-src prev">${esc(t('Your own build'))}</span>`;
    const acts = `<button type="button" class="btn sm" data-ss="owncopy"${ns.busy ? ' disabled' : ''}>${esc(t('Use My Copy'))}</button><button type="button" class="btn sm" data-ss="ownpick"${ns.busy ? ' disabled' : ''}>${esc(t('Choose Folder'))}</button>`;
    return `<div class="ss-box ${bad ? 'warn' : 'on'}"><span class="ss-ic">${bad ? SS_WARN : CHECK}</span><div class="k"><b>${esc(title)}</b><span title="${esc(ns.own)}">${esc(line)}</span></div><div class="low"><div class="meta">${tag}</div><div class="acts">${acts}</div></div></div>`;
  }
  const on = !!r.path;
  const line = on ? baseName(r.path) : r.label;
  const meta = on ? ssTag(r) : `<span class="fmts">${ssFmts(r)}</span>`;
  const icon = on ? CHECK : r.file ? DISC_L : SS_FOLDER;
  return `<div class="ss-box${on ? ' on' : ''}"><span class="ss-ic">${icon}</span><div class="k"><b>${esc(title)}</b><span title="${esc(on ? r.path : r.label)}">${esc(line)}</span></div><div class="low"><div class="meta">${meta}</div><div class="acts">${ssPickBtns(r, ns.busy)}</div></div></div>`;
}

// A compact row: discs, keys, updates, add-ons. Remove keeps its place while the row is empty.
function ssRow(r, ns) {
  const on = !!r.path;
  const sub = on ? (r.shared ? t('Kept once on this PC for every game that needs it') : baseName(r.path)) : r.legacy ? '' : ssFmts(r) ? '' : t('Not chosen yet');
  const fmts = !on && ssFmts(r) ? `<span class="fmts">${ssFmts(r)}</span>` : '';
  const canRemove = r.optional || (r.shared && !r.legacy);
  const rm = canRemove ? `<button type="button" class="btn sm rm" data-ss="${r.shared ? 'remove-key' : 'clear'}" data-input="${esc(r.id)}" aria-label="${esc(t('Remove') + ': ' + r.label)}" title="${esc(t('Remove'))}"${on && !ns.busy ? '' : ' disabled'}>${XMARK}</button>` : '';
  // A shared key's path is the launcher's private copy: never shown, not even as a tooltip.
  const tip = r.shared ? '' : ` title="${esc(r.path || '')}"`;
  return `<div class="ss-row${on ? ' on' : ''}${r.optional ? ' opt' : ''}"><span class="st">${CHECK}</span><div class="k"><b>${esc(r.label)}</b><div class="line">${sub ? `<span${tip}>${esc(sub)}</span>` : ''}${fmts}${on ? ssTag(r) : ''}</div></div><div class="acts">${ssPickBtns(r, ns.busy)}${rm}</div></div>`;
}

// Extras: the setup step's options (switches; one with a license starts off and the player turns
// it on) and NVIDIA DLSS, which the launcher fetches itself.
function ssOption(o, skipBuild) {
  if (skipBuild) return `<label class="su-opt off"><span class="su-opt-text"><b>${esc(o.label)}</b><small>${esc(t('Only when the launcher builds the game from your copy'))}</small></span><input type="checkbox" role="switch" data-popt="${esc(o.id)}" disabled${o.on ? ' checked' : ''}><span class="switch" aria-hidden="true"></span></label>`;
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
  const rail = m.steps.length > 1 ? `<div class="ss-rail">${m.steps.map((s, i) => `${i ? '<span class="bar"></span>' : ''}<span class="s${i === m.at ? ' now' : i < m.at ? ' done' : ''}${s.id === 'extras' && m.reqOff ? ' req' : ''}${s.id === 'extras' && m.fresh.length ? ' new' : ''}"><i>${i < m.at ? CHECK : i + 1}</i>${esc(s.name)}</span>`).join('')}</div>` : '';
  let head = '', lead = '', body = '', status = '';
  if (m.cur.id === 'game') {
    head = m.own.filter((r) => r.role === 'disc').length > 1 ? t('Insert your discs') : t('Insert your copy');
    lead = t('Checked once when it\'s set up. After that, you just press Play.');
    // Built again from what its setup step kept: the copy's box stays, for building from it instead.
    const game = m.own.find((r) => r.legacy && r.id === 'game');
    if (m.kept && game && !game.path) {
      head = t('Build It Again');
      lead = t('Your copy isn\'t needed: it\'s built again from what it kept last time.');
    }
    const one = m.own.length === 1 && (m.own[0].legacy || m.own[0].role === 'game' || m.own[0].role === 'disc');
    const title = m.own[0] && m.own[0].legacy ? (game.game || name) : m.own[0] ? m.own[0].label : '';
    const main = one ? ssBox(id, su, ns, m.own[0], title) : m.own.length ? `<div class="ss-rows">${m.own.map((r) => ssRow(r, ns)).join('')}</div>` : '';
    const keys = m.keys.length ? `<div><p class="ss-cap">${esc(t('Keys'))} <i>${esc(t('kept once on this PC, for every game that needs them'))}</i></p><div class="ss-rows">${m.keys.map((r) => ssRow(r, ns)).join('')}</div></div>` : '';
    body = main + keys + ssAlt(su, ns, name);
    status = m.missing.length ? tn(m.missing.length, 'Choose 1 more file to go on.', 'Choose {n} more files to go on.') : t('Everything it needs is here.');
    if (ns.own && ns.ownErr) status = t('Choose the finished game\'s folder.');
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
    if (m.skipBuild && m.opts.length) lead = t('Build options only apply when the game is built from your copy.');
    body = `<div class="ss-opts su-opts">${m.opts.map((o) => ssOption(o, m.skipBuild)).join('')}${m.runtime.map((r) => ssRuntime(r, ns)).join('')}</div>`;
  }
  if (m.last) {
    body += ssSummary(su, ns, m);
    if (m.cur.id !== 'game') status = m.missing.length ? tn(m.missing.length, '1 file missing on step 1.', '{n} files missing on step 1.') : m.reqOff ? prepReq() : t('Ready to set up.');
    else if (!m.missing.length) status = m.reqOff ? prepReq() : t('Ready to set up.');
    if (m.ownBad) status = t('Choose the finished game\'s folder.');
  }
  // The check mark only on the lines that say it's ready.
  const ok = [t('Everything it needs is here.'), t('Ready to set up.')].includes(status);
  const error = ns.error || su.inputs_error;
  const notice = ns.confirm ? `<div class="ni-notice"><p>${esc(t('Removing shared keys affects every game that uses them. Your original file stays where it is.'))}</p><button type="button" class="btn sm" data-ss="remove-confirm" data-input="${esc(ns.confirm)}">${esc(t('Remove shared keys'))}</button> <button type="button" class="btn sm" data-ss="cancel-remove">${esc(t('Keep'))}</button></div>` : '';
  // Back keeps its place on the first step too (hidden), so the footer doesn't move between steps.
  const back = m.steps.length > 1 ? `<button type="button" class="btn big${m.at ? '' : ' ghost'}" data-ss="back"${m.at ? '' : ' tabindex="-1" aria-hidden="true" disabled'}>${esc(t('Back'))}</button>` : '';
  const next = m.last ? `<button type="button" class="btn main big" data-ss="run"${m.blocked ? ' disabled' : ''}>${esc(ns.busy ? t('Working…') : t('Set Up'))}</button>`
    : `<button type="button" class="btn main big" data-ss="next"${m.cur.id === 'game' && (m.missing.length || m.broken) ? ' disabled' : ''}>${esc(t('Next'))}</button>`;
  return `<div class="ss-head"><span class="kick">${esc(t('Set up'))} · ${esc(name)}</span>${rail}<h2>${esc(head)}</h2><p>${esc(lead)}</p></div>
    <div class="ss-scroll">${body}${notice}${error ? `<p class="su-err" role="alert">${esc(t(error))}</p>` : ''}</div>
    <div class="ss-foot">${back}<p class="why" role="status">${ok ? CHECK : ''}${esc(status)}</p>${next}</div>`;
}

// The quiet line under the game's box: a build that's already there, or the files from before.
function ssAlt(su, ns, name) {
  if (ns.own) return '';
  if (su.prebuilt && !ns.rebuild) return `<p class="ss-alt">${esc(t('The game built from your copy last time is still here, so setup skips the build.'))} <button type="button" data-ss="rebuild">${esc(t('Build It Again'))}</button></p>`;
  if (su.prebuilt) return `<p class="ss-alt">${esc(t('Building it again from your copy.'))} <button type="button" data-ss="keepbuild">${esc(t('Use the One Still Here'))}</button></p>`;
  const prev = su.previous || [];
  if (prev.length && !su.inputs_ready) return `<p class="ss-alt">${esc(tf('Set up before with {files}.', { files: prev.join(' · ') }))} <button type="button" data-ss="relink">${esc(t('Use Them Again'))}</button></p>`;
  if (su.own_build) return `<p class="ss-alt">${esc(tf('Already built {name} from your copy, outside the launcher? Skip the build:', { name }))} <button type="button" data-ss="ownpick">${esc(t('Choose That Folder'))}</button></p>`;
  return '';
}

// The last step sums up, and says what Set Up starts: the developer's own prepare.label.
function ssSummary(su, ns, m) {
  const files = ns.own ? t('Your own build') : tf('{ready} of {total} ready', { ready: m.own.concat(m.keys).filter((r) => r.path).length, total: m.own.length + m.keys.length });
  const addons = m.addons.length ? `<div><span>${esc(t('Add-ons'))}</span><b>${esc(tf('{added} of {total} added', { added: m.addons.filter((r) => r.path).length, total: m.addons.length }))}</b></div>` : '';
  const then = ns.own ? t('Nothing to build: it uses the build you chose') : m.skipBuild ? t('Nothing to build: the game built last time is still here') : su.prepare_label || '';
  // A setup that builds outside the game's own folder says where (build_where), as it always has.
  const where = su.build_where && !m.skipBuild ? `<div><span>${esc(t('Builds into'))}</span><b>${esc(su.build_where)}</b></div>` : '';
  // On a one-page setup the box above already shows the files: only what Set Up starts.
  const count = m.steps.length > 1 ? `<div><span>${esc(t('Game files'))}</span><b>${esc(files)}</b></div>${addons}` : '';
  const rows = `${count}${then ? `<div><span>${esc(t('Then'))}</span><b>${esc(then)}</b></div>` : ''}${where}`;
  return rows ? `<div class="ss-sum">${rows}</div>` : '';
}

// The left side: the game's media, ghosted (with the drop hint on the desktop) until a file is in.
function stepsShell(id, su, ns, game, tv) {
  const m = stepsModel(id, su, ns);
  const any = !!ns.own || m.own.some((r) => r.path);
  const discs = m.own.filter((r) => r.role === 'disc');
  const sum = discs.length > 1 ? `<p class="ni-disc-summary" role="status">${esc(tf('{ready} of {total} discs selected', { ready: discs.filter((r) => r.path).length, total: discs.length }))}</p>` : '';
  const art = gameMediaHtml(game, any ? '' : 'ghost');
  if (tv) return `<div class="ni-tv-layout"><div class="ni-tv-art"><div class="tv-media" aria-hidden="true">${art}</div>${sum}</div><div class="ni-tv su-r ss">${stepsView(id, su, ns, game)}</div></div>`;
  const drop = any ? `<div class="su-media">${art}</div>` : `<div class="su-drop"><div class="su-media">${art}</div><b>${esc(copyDrop(copyFormats(id)))}</b><span>${esc(t('or choose it on the right'))}</span></div>`;
  return `<div class="su-in ss-in"><div class="su-stage">${drop}<div class="drive" aria-hidden="true"><i></i></div>${sum}</div><div class="su-r ss">${stepsView(id, su, ns, game)}</div></div>`;
}

// What a button does. Most keep the window as it is and redraw; 'run' is the host's to start.
async function stepsAction(id, act, input, ns) {
  const su = S.setup[id];
  const row = input ? stepsRows(id, su, ns).find((r) => r.id === input) : null;
  if (act === 'next') { ns.at = (ns.at || 0) + 1; return; }
  if (act === 'back') { ns.at = Math.max(0, (ns.at || 0) - 1); return; }
  if (act === 'rebuild') { ns.rebuild = true; return; }
  if (act === 'keepbuild') { ns.rebuild = false; return; }
  if (act === 'owncopy') { ns.own = ''; ns.ownErr = ''; return; }
  if (act === 'run') return 'run';
  if (act === 'ownpick') {
    const p = await pickFile({ title: tf('The folder you built {name} into', { name: (S.library[id] || {}).name || id }), directory: true }).catch(fail);
    if (p) { ns.own = typeof p === 'string' ? p : p.path; ns.ownErr = ''; }
    return;
  }
  if (act === 'relink') {
    ns.busy = true;
    try {
      const r = await relinkSetup(id);
      if (r === 'ready') return 'ready';
      if (r && r.label) ns.error = tf('{file} isn\'t where it was. Choose it again, then press Set Up.', { file: r.label });
    } catch (e) { ns.error = String(e && e.message ? e.message : e); }
    finally { ns.busy = false; }
    return;
  }
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
    await invoke('setup_input_set', { id, input: empty.id, path });
    S.setup[id] = await invoke('game_setup', { id });
    ns.picked = Object.assign({}, ns.picked, { [empty.id]: true });
  } catch (e) { ns.error = String(e && e.message ? e.message : e); }
}

// Set Up: what starts, by what the screen has. { ready: text } when nothing has to run,
// { copy: path } for setup_copy, { kept: true } for setup_copy without the copy (built again from
// what it kept), { inputs: true } for setup_inputs_run, or nothing when it
// couldn't (the reason is in ns). saving: the options' save still on its way, which the backend
// checks the required ones against.
async function stepsRun(id, ns, saving) {
  const su = S.setup[id];
  const m = stepsModel(id, su, ns);
  if (m.blocked) return null;
  ns.error = '';
  if (ns.own || m.skipBuild) {
    ns.busy = true;
    try {
      await invoke('setup_use_build', ns.own ? { id, folder: ns.own } : { id });
      S.setup[id] = await invoke('game_setup', { id });
      await refreshState();
      return { ready: ns.own ? t('Your own build') : t('The build on this PC') };
    } catch (e) {
      const msg = String(e && e.message ? e.message : e);
      if (ns.own) ns.ownErr = msg; else ns.error = msg;
      return null;
    } finally { ns.busy = false; }
  }
  await saving;
  if ((su.inputs || []).length) return { inputs: true };
  const game = m.own.find((r) => r.legacy && r.id === 'game');
  if (game && game.path) return { copy: game.path };
  return m.kept ? { kept: true } : null;
}
