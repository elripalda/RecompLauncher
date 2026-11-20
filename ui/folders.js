'use strict';
// Folder Structure (Settings → Games and folders): the one folder layout games go in on disk,
// designed by dragging fields into folder levels, with a live preview, and Reorganize Existing
// Folders to move installed games into it. Only where games sit on disk changes: the Library,
// its sorting and the game pages stay as they are. The rules for names, fallbacks and moves live
// in the backend (folders.rs, folders_move.rs), so the preview is exactly what installs get.

const FS = { data: null, edit: null, level: 0, preview: [], timer: 0, drag: null, busy: false, loading: false };

// Each field: its name, and what the launcher actually has for it (checked against the catalog
// and the Library), shown on its chip and in Fallbacks.
const FS_FIELDS = {
  maker: [N_('Manufacturer'), N_('From the console (a built-in list)')],
  console: [N_('Console'), N_('From the catalog; games you added from GitHub have none')],
  game: [N_('Game Title'), N_('From the catalog, or what the Library noted')],
  recomp: [N_('Recompilation'), N_('Always known')],
  franchise: [N_('Franchise'), N_('Not in the catalog: only what you enter per game')],
  year: [N_('Original Release Year'), N_('Not in the catalog: only what you enter per game')],
  recomp_year: [N_('Recompilation Year'), N_('The year of its first release, from the catalog')],
  developer: [N_('Original Developer'), N_('Not in the catalog: only what you enter per game')],
  team: [N_('Recompilation Team'), N_('Its developer page or project owner')],
  publisher: [N_('Publisher'), N_('Not in the catalog: only what you enter per game')],
  genre: [N_('Genre'), N_('From the catalog, for most games')],
  region: [N_('Region'), N_('Only for an N64 copy you linked')],
  tech: [N_('Technology'), N_('From the catalog\'s tags (decomp, port…), when it has one')],
  source: [N_('Installation Source'), N_('Always known (GitHub, GitLab, local build…)')],
  letter: [N_('Alphabetical'), N_('The first letter of the game\'s title')],
  version: [N_('Version'), N_('Always known. It changes with updates, so reorganizing again would rename the folder')],
  text: [N_('Custom Text'), N_('Words you type, the same for every game')],
};
// Fields the player can give a value per game (Advanced → Your Values).
const FS_OWN = ['game', 'franchise', 'year', 'developer', 'publisher', 'region', 'console', 'genre', 'tech'];
const FS_SEPS = [[' - ', ' - '], [' ', N_('Space')], [' – ', ' – '], ['_', '_'], [', ', ', '], [' · ', ' · ']];
const FS_CASES = [['keep', N_('As written')], ['title', N_('Title Case')], ['upper', N_('UPPER CASE')], ['lower', N_('lower case')]];

const fsLabel = (f) => t((FS_FIELDS[f] || [f])[0]);
const fsHint = (f) => t((FS_FIELDS[f] || ['', ''])[1]);
const fsClone = (x) => JSON.parse(JSON.stringify(x));
const fsPresetName = (id) => { const p = ((FS.data || {}).presets || []).find((x) => x.id === id); return p ? t(p.name) : t('Your own'); };
const fsSummary = (layout) => (layout.levels || []).map((l) => l.parts.map((p) => (p.field === 'text' ? `"${p.text}"` : fsLabel(p.field))).join(' + ')).join(' → ');

async function fsLoad() {
  if (FS.loading) return;
  FS.loading = true;
  try { FS.data = await invoke('folders_get'); } catch (e) { FS.data = null; }
  FS.loading = false;
  if (S.view === 'settings') render(true);
}

// Settings → Games and folders → Folder Structure.
function folderStructureGroup(group, row, sw) {
  if (!FS.data) { fsLoad(); return group(t('Folder Structure'), `<p class="set-note">${esc(t('Loading…'))}</p>`); }
  const L = FS.data.layout;
  const markup = (b, s, ctl) => `<div class="set-row"><div class="k"><b>${esc(b)}</b><span>${s}</span></div>${ctl}</div>`;
  const design = markup(t('Your Folder Structure'), `${esc(fsSummary(L))} <span class="fs-dim">· ${esc(fsPresetName(L.preset))}</span>`,
    `<button type="button" class="btn sm main" data-act="fs-design">${esc(t('Design…'))}</button>`);
  const apply = row(t('Automatically use my folder structure for new installations'), t('On: new games go in its folders. Off: they go where they always have, a folder named after the game in the library. Updates always stay in the game\'s folder.'),
    sw('fs_new', !!L.apply_new, t('Automatically use my folder structure for new installations'), 'fs-new'));
  const undo = FS.data.undo ? `<button type="button" class="btn sm" data-act="fs-undo">${esc(t('Undo Last Reorganize'))}</button>` : '';
  const reorg = row(t('Reorganize Existing Folders'), t('Shows where each installed game is and where your structure would put it, then moves the ones you tick. Nothing moves until you confirm.'),
    `<span class="acts">${undo}<button type="button" class="btn sm" data-act="fs-reorg">${esc(t('Reorganize…'))}</button></span>`);
  return group(t('Folder Structure'), design + apply + reorg);
}
window.folderStructureGroup = folderStructureGroup;

window.foldersAct = async (a) => {
  if (!FS.data) await fsLoad();
  if (a === 'fs-design') return fsDesigner();
  if (a === 'fs-reorg') return fsReorganize();
  if (a === 'fs-undo') return fsUndo();
  if (a === 'fs-new') {
    const next = Object.assign(fsClone(FS.data.layout), { apply_new: !FS.data.layout.apply_new });
    try { FS.data.layout = await invoke('folders_save', { layout: next }); } catch (e) { return fail(e); }
    render(true);
  }
};

// ---------- the designer ----------

function fsChip(f, text, where) {
  const label = f === 'text' ? (text ? `"${text}"` : t('Custom Text')) : fsLabel(f);
  return `<span class="fs-chip${f === 'text' ? ' txt' : ''}" draggable="true" data-drag="${esc(where)}" title="${esc(fsHint(f))}">${esc(label)}${where.startsWith('part:') ? `<button type="button" class="fs-x" data-x="part-del" data-at="${esc(where)}" aria-label="${esc(tf('Take {field} out', { field: label }))}">×</button>` : ''}</span>`;
}

function fsLevel(l, i, n) {
  const sel = FS.level === i;
  const seps = FS_SEPS.map(([v, name]) => `<option value="${esc(v)}"${l.sep === v ? ' selected' : ''}>${esc(t(name))}</option>`).join('');
  const custom = l.parts.map((p, pi) => (p.field === 'text' ? `<input class="field sm fs-txt" data-txt="${i}:${pi}" value="${esc(p.text || '')}" placeholder="${esc(t('Folder words'))}" aria-label="${esc(t('Custom Text'))}">` : '')).join('');
  return `<div class="fs-level${sel ? ' fs-on' : ''}" data-level="${i}" data-drop="level:${i}">
    <div class="fs-lh"><button type="button" class="fs-pick" data-x="pick" data-i="${i}" aria-pressed="${sel}">${esc(i === n - 1 ? t('The game\'s folder') : tf('Folder {n}', { n: i + 1 }))}</button>
      <span class="fs-tools">
        ${l.parts.length > 1 ? `<label class="fs-sep">${esc(t('Join with'))} <select class="sel sm" data-sep="${i}">${seps}</select></label>` : ''}
        <button type="button" class="btn xs" data-x="up" data-i="${i}" ${i === 0 ? 'disabled' : ''} aria-label="${esc(t('Move this level up'))}">↑</button>
        <button type="button" class="btn xs" data-x="down" data-i="${i}" ${i === n - 1 ? 'disabled' : ''} aria-label="${esc(t('Move this level down'))}">↓</button>
        <button type="button" class="btn xs" data-x="del" data-i="${i}" ${n === 1 ? 'disabled' : ''} aria-label="${esc(t('Remove this level'))}">${esc(t('Remove'))}</button>
      </span></div>
    <div class="fs-parts">${l.parts.map((p, pi) => fsChip(p.field, p.text, `part:${i}:${pi}`)).join('') || `<span class="fs-empty">${esc(t('Drag fields here, or pick this level and press a field.'))}</span>`}</div>
    ${custom ? `<div class="fs-txts">${custom}</div>` : ''}
  </div>`;
}

// The preview as a tree: shared parent folders once, each game's folder under them.
function fsTree(rows) {
  if (!rows.length) return `<p class="muted">${esc(t('Nothing to show yet.'))}</p>`;
  const byLib = new Map();
  for (const r of rows) { const k = r.example ? t('Examples') : r.library; if (!byLib.has(k)) byLib.set(k, []); byLib.get(k).push(r); }
  let out = '';
  for (const [lib, list] of byLib) {
    out += `<div class="fs-root">${esc(lib)}</div>`;
    let prev = [];
    for (const r of list) {
      const f = r.folders;
      let same = 0;
      while (same < f.length - 1 && same < prev.length && prev[same] === f[same]) same++;
      for (let d = same; d < f.length; d++) {
        const last = d === f.length - 1;
        out += `<div class="fs-node${last ? ' game' : ''}" style="--d:${d}">${esc(f[d])}/${last && r.example ? ` <small>${esc(t('example'))}</small>` : ''}</div>`;
      }
      prev = f;
    }
  }
  return out;
}

function fsAdvanced(L) {
  const used = [...new Set(L.levels.flatMap((l) => l.parts.map((p) => p.field)))].filter((f) => !['recomp', 'text', 'source', 'letter', 'version'].includes(f));
  const fbRows = used.map((f) => {
    const fb = (L.fallbacks || {})[f] || FS.data.fallbacks[f] || { mode: 'skip' };
    const modes = [['text', t('Use words')], ['skip', t('Leave it out')], ['field', t('Use another field')]].map(([v, n]) => `<option value="${v}"${fb.mode === v ? ' selected' : ''}>${esc(n)}</option>`).join('');
    const other = Object.keys(FS_FIELDS).filter((x) => x !== f && x !== 'text').map((x) => `<option value="${x}"${fb.field === x ? ' selected' : ''}>${esc(fsLabel(x))}</option>`).join('');
    const extra = fb.mode === 'text' ? `<input class="field sm" data-fbtext="${f}" value="${esc(fb.text || '')}" aria-label="${esc(tf('Words for a missing {field}', { field: fsLabel(f) }))}">` : fb.mode === 'field' ? `<select class="sel sm" data-fbfield="${f}" aria-label="${esc(t('Field to use'))}">${other}</select>` : '';
    return `<div class="fs-row"><span><b>${esc(fsLabel(f))}</b><small>${esc(fsHint(f))}</small></span><span class="acts"><select class="sel sm" data-fbmode="${f}" aria-label="${esc(tf('When a game has no {field}', { field: fsLabel(f) }))}">${modes}</select>${extra}</span></div>`;
  }).join('');
  const cases = FS_CASES.map(([v, n]) => `<option value="${v}"${L.case === v ? ' selected' : ''}>${esc(t(n))}</option>`).join('');
  const cons = (FS.data.consoles || []).filter((c) => c.id !== 'other').map((c) => `<label class="fs-con"><span>${esc(c.full)}</span><input class="field sm" data-con="${esc(c.id)}" value="${esc((L.consoles || {})[c.id] || '')}" placeholder="${esc(L.short_consoles ? c.short : c.full)}"></label>`).join('');
  const games = Object.values(S.library || {}).filter((g) => !g.local).sort((x, y) => x.name.localeCompare(y.name));
  const ownFields = FS_OWN.filter((f) => used.includes(f) || ['franchise', 'year'].includes(f));
  const own = games.length ? `<div class="fs-own"><div class="fs-own-h"><span>${esc(t('Game'))}</span>${ownFields.map((f) => `<span>${esc(fsLabel(f))}</span>`).join('')}</div>
    ${games.map((g) => `<div class="fs-own-r"><span title="${esc(g.name)}">${esc(g.name)}</span>${ownFields.map((f) => `<input class="field sm" data-own="${esc(g.id)}:${f}" value="${esc(((L.games || {})[g.id] || {})[f] || '')}" aria-label="${esc(tf('{field} for {game}', { field: fsLabel(f), game: g.name }))}">`).join('')}</div>`).join('')}</div>`
    : `<p class="muted">${esc(t('Install a game to give it your own values.'))}</p>`;
  return `<details class="fs-adv"${FS.adv ? ' open' : ''}><summary data-x="adv">${esc(t('Advanced'))}</summary>
    <h4>${esc(t('When a game has nothing for a field'))}</h4>
    <p class="fs-dim">${esc(t('The catalog doesn\'t know everything (no publisher or original release year, for one). Pick what goes in the folder name instead.'))}</p>
    ${fbRows || `<p class="muted">${esc(t('Every field in your structure is always known.'))}</p>`}
    <h4>${esc(t('Names'))}</h4>
    <div class="fs-row"><span><b>${esc(t('Console Names'))}</b></span><span class="acts"><select class="sel sm" data-short aria-label="${esc(t('Console Names'))}"><option value="1"${L.short_consoles ? ' selected' : ''}>${esc(t('Short (N64, PS2)'))}</option><option value="0"${L.short_consoles ? '' : ' selected'}>${esc(t('Full (Nintendo 64, PlayStation 2)'))}</option></select></span></div>
    <div class="fs-row"><span><b>${esc(t('Capitalization'))}</b></span><span class="acts"><select class="sel sm" data-case aria-label="${esc(t('Capitalization'))}">${cases}</select></span></div>
    <details class="fs-sub"><summary>${esc(t('Your own console names'))}</summary><div class="fs-cons">${cons}</div></details>
    <h4>${esc(t('Your values per game'))}</h4>
    <p class="fs-dim">${esc(t('Fill in what the catalog doesn\'t have. These are kept, so refreshing the catalog never resets them.'))}</p>
    ${own}
  </details>`;
}

function fsDraw() {
  const L = FS.edit, n = L.levels.length;
  const presets = FS.data.presets.map((p) => `<option value="${esc(p.id)}"${L.preset === p.id ? ' selected' : ''}>${esc(t(p.name))}</option>`).join('');
  const palette = Object.keys(FS_FIELDS).map((f) => `<button type="button" class="fs-add" data-x="add" data-f="${f}" draggable="true" data-drag="field:${f}" title="${esc(fsHint(f))}">${esc(fsLabel(f))}</button>`).join('');
  return `<h2>${esc(t('Folder Structure'))}</h2>
    <p>${esc(t('Where installed games go on disk. Your Library, its sorting and game pages don\'t change.'))}</p>
    <div class="fs-step"><b>1</b><label>${esc(t('Start from'))} <select class="sel" data-preset aria-label="${esc(t('Start from'))}">${presets}${L.preset ? '' : `<option selected>${esc(t('Your own'))}</option>`}</select></label></div>
    <div class="fs-grid">
      <div class="fs-pal" data-drop="palette"><div class="fs-h"><b>2</b> ${esc(t('Fields'))}</div><p class="fs-dim">${esc(t('Drag one onto a folder level, or press it to add it to the level you picked. Drag a field back here to take it out.'))}</p><div class="fs-adds">${palette}</div></div>
      <div class="fs-levels"><div class="fs-h">${esc(t('Folders, outermost first'))}</div>${L.levels.map((l, i) => fsLevel(l, i, n) + (i < n - 1 ? '<div class="fs-arrow" aria-hidden="true">↓</div>' : '')).join('')}
        <button type="button" class="btn sm" data-x="level-add">${esc(t('Add a Level'))}</button></div>
      <div class="fs-prev"><div class="fs-h"><b>3</b> ${esc(t('Preview'))}</div><div class="fs-tree" aria-live="polite">${fsTree(FS.preview)}</div></div>
    </div>
    ${fsAdvanced(L)}
    <div class="acts"><button type="button" class="btn" data-x="reset">${esc(t('Restore Default'))}</button><button type="button" class="btn" data-x="no">${esc(t('Cancel'))}</button><button type="button" class="btn main" data-x="save">${esc(t('Save'))}</button></div>`;
}

function fsPreviewSoon(box) {
  clearTimeout(FS.timer);
  FS.timer = setTimeout(async () => {
    try { FS.preview = await invoke('folders_preview', { layout: FS.edit }); } catch (e) { return; }
    const tree = box && box.querySelector('.fs-tree');
    if (tree) tree.innerHTML = fsTree(FS.preview);
  }, 120);
}

// Redrawn after each change; the focused control gets its focus back.
function fsRedraw(box) {
  const f = document.activeElement, key = f && box.contains(f) ? ['data-x', 'data-i', 'data-f', 'data-sep', 'data-txt', 'data-fbtext', 'data-own', 'data-con'].map((k) => (f.getAttribute(k) != null ? `[${k}="${CSS.escape(f.getAttribute(k))}"]` : '')).join('') : '';
  FS.adv = !!(box.querySelector('.fs-adv') || {}).open;
  box.innerHTML = fsDraw();
  if (key) { const back = box.querySelector(key); if (back) back.focus(); }
  fsPreviewSoon(box);
}

// A change that isn't one of the presets any more is the player's own structure.
const fsChanged = () => { FS.edit.preset = ''; };

function fsMovePart(from, toLevel, toIndex) {
  const [, li, pi] = from.split(':').map(Number);
  const part = FS.edit.levels[li].parts.splice(pi, 1)[0];
  const dest = FS.edit.levels[toLevel].parts;
  const at = toLevel === li && toIndex > pi ? toIndex - 1 : toIndex;
  dest.splice(Math.min(at, dest.length), 0, part);
}

function fsDrop(box, drag, target, before) {
  if (!drag || !target) return;
  if (target === 'palette') {
    if (drag.startsWith('part:')) { const [, li, pi] = drag.split(':').map(Number); FS.edit.levels[li].parts.splice(pi, 1); fsChanged(); fsRedraw(box); }
    return;
  }
  const li = Number(target.split(':')[1]);
  const at = before != null ? before : FS.edit.levels[li].parts.length;
  if (drag.startsWith('field:')) FS.edit.levels[li].parts.splice(at, 0, { field: drag.slice(6), text: '' });
  else fsMovePart(drag, li, at);
  FS.level = li;
  fsChanged();
  fsRedraw(box);
}

function fsWire(box) {
  box.addEventListener('dragstart', (e) => { const d = e.target.closest('[data-drag]'); if (!d) return; FS.drag = d.dataset.drag; e.dataTransfer.effectAllowed = 'move'; e.dataTransfer.setData('text/plain', FS.drag); });
  box.addEventListener('dragover', (e) => { const z = e.target.closest('[data-drop]'); if (!z) return; e.preventDefault(); for (const x of box.querySelectorAll('.drop-on')) x.classList.remove('drop-on'); z.classList.add('drop-on'); });
  box.addEventListener('dragend', () => { FS.drag = null; for (const x of box.querySelectorAll('.drop-on')) x.classList.remove('drop-on'); });
  box.addEventListener('drop', (e) => {
    const z = e.target.closest('[data-drop]');
    if (!z) return;
    e.preventDefault();
    // Dropped on a chip in a level: it goes before that chip.
    const chip = e.target.closest('.fs-chip[data-drag^="part:"]');
    const before = chip && z.dataset.drop.startsWith('level:') ? Number(chip.dataset.drag.split(':')[2]) : null;
    fsDrop(box, FS.drag || e.dataTransfer.getData('text/plain'), z.dataset.drop, before);
    FS.drag = null;
  });
  box.addEventListener('change', (e) => {
    const el = e.target, L = FS.edit;
    if (el.matches('[data-preset]')) return invoke('folders_preset', { id: el.value }).then((p) => { FS.edit = Object.assign(p, { apply_new: L.apply_new, games: L.games, consoles: L.consoles, fallbacks: L.fallbacks, short_consoles: L.short_consoles, case: L.case }); FS.level = 0; fsRedraw(box); }).catch(fail);
    if (el.dataset.sep != null) { L.levels[Number(el.dataset.sep)].sep = el.value; fsChanged(); }
    else if (el.dataset.fbmode) { const f = el.dataset.fbmode; L.fallbacks = L.fallbacks || {}; L.fallbacks[f] = { mode: el.value, text: el.value === 'text' ? t('Unknown') : '', field: el.value === 'field' ? 'recomp' : '' }; }
    else if (el.dataset.fbfield) { L.fallbacks[el.dataset.fbfield].field = el.value; }
    else if (el.matches('[data-short]')) L.short_consoles = el.value === '1';
    else if (el.matches('[data-case]')) L.case = el.value;
    else return;
    fsRedraw(box);
  });
  box.addEventListener('input', (e) => {
    const el = e.target, L = FS.edit;
    if (el.dataset.txt) { const [li, pi] = el.dataset.txt.split(':').map(Number); L.levels[li].parts[pi].text = el.value; fsChanged(); }
    else if (el.dataset.fbtext) L.fallbacks[el.dataset.fbtext].text = el.value;
    else if (el.dataset.con) { L.consoles = L.consoles || {}; if (el.value.trim()) L.consoles[el.dataset.con] = el.value; else delete L.consoles[el.dataset.con]; }
    else if (el.dataset.own) {
      const [id, f] = [el.dataset.own.slice(0, el.dataset.own.lastIndexOf(':')), el.dataset.own.slice(el.dataset.own.lastIndexOf(':') + 1)];
      L.games = L.games || {};
      const g = L.games[id] = L.games[id] || {};
      if (el.value.trim()) g[f] = el.value; else delete g[f];
      if (!Object.keys(g).length) delete L.games[id];
    } else return;
    // Typing doesn't redraw (it would lose the caret): only the preview follows.
    fsPreviewSoon(box);
  });
}

async function fsDesigner() {
  if (!FS.data) return;
  FS.edit = fsClone(FS.data.layout);
  FS.level = 0;
  FS.preview = [];
  let box = null;
  const scrim = modal('<div class="fs-designer"></div>', async (x, close) => {
    const L = FS.edit, i = Number(x.dataset.i);
    switch (x.dataset.x) {
      case 'pick': FS.level = i; break;
      case 'add': L.levels[Math.min(FS.level, L.levels.length - 1)].parts.push({ field: x.dataset.f, text: '' }); fsChanged(); break;
      case 'part-del': { const [, li, pi] = x.dataset.at.split(':').map(Number); L.levels[li].parts.splice(pi, 1); fsChanged(); break; }
      case 'up': [L.levels[i - 1], L.levels[i]] = [L.levels[i], L.levels[i - 1]]; FS.level = i - 1; fsChanged(); break;
      case 'down': [L.levels[i + 1], L.levels[i]] = [L.levels[i], L.levels[i + 1]]; FS.level = i + 1; fsChanged(); break;
      case 'del': L.levels.splice(i, 1); FS.level = Math.max(0, Math.min(FS.level, L.levels.length - 1)); fsChanged(); break;
      case 'level-add': L.levels.splice(L.levels.length - 1, 0, { parts: [], sep: ' - ' }); FS.level = Math.max(0, L.levels.length - 2); fsChanged(); break;
      case 'reset': try { FS.edit = Object.assign(await invoke('folders_preset', { id: 'console-game-recomp' }), { apply_new: L.apply_new }); FS.level = 0; } catch (e) { return fail(e); } break;
      case 'adv': return;
      case 'save': {
        const empty = L.levels.findIndex((l) => !l.parts.length);
        if (empty >= 0) return toast(tf('Folder {n} is empty: add a field or remove it.', { n: empty + 1 }), true);
        try { FS.data.layout = await invoke('folders_save', { layout: L }); } catch (e) { return fail(e); }
        close();
        toast(t('Folder structure saved. Games already installed stay where they are until you reorganize them.'));
        return render(true);
      }
      default: return;
    }
    fsRedraw(box);
  }, t('Folder Structure'));
  box = scrim.querySelector('.modal');
  box.classList.add('fs-modal');
  box.innerHTML = fsDraw();
  fsWire(box);
  fsPreviewSoon(box);
}

// ---------- Reorganize Existing Folders ----------

const FS_STATE = { move: N_('Moves'), same: N_('Already there'), outside: N_('Outside your libraries'), blocked: N_('Stays') };

async function fsReorganize() {
  let rows;
  try { rows = await invoke('folders_plan'); } catch (e) { return fail(e); }
  const pick = new Set(rows.filter((r) => r.on).map((r) => r.id));
  const movable = (r) => r.state === 'move' || r.state === 'outside';
  const draw = () => {
    const n = rows.filter((r) => movable(r) && pick.has(r.id)).length;
    const list = rows.map((r) => `<div class="fs-plan${movable(r) ? '' : ' off'}">
      <label><input type="checkbox" data-id="${esc(r.id)}"${pick.has(r.id) ? ' checked' : ''}${movable(r) ? '' : ' disabled'} aria-label="${esc(tf('Move {game}', { game: r.name }))}"><b>${esc(r.name)}</b><span class="fs-tag ${esc(r.state)}">${esc(t(FS_STATE[r.state] || r.state))}</span></label>
      <div class="fs-paths"><span><small>${esc(t('Now'))}</small><code>${esc(r.from)}</code></span><span><small>${esc(t('New'))}</small><code>${esc(r.state === 'blocked' ? '—' : r.to)}</code></span></div>
      ${r.note ? `<p class="fs-note">${esc(t(r.note))}</p>` : ''}</div>`).join('');
    return `<h2>${esc(t('Reorganize Existing Folders'))}</h2>
      <p>${esc(t('Each game\'s folder moves whole, with the saves and settings inside it. Nothing is overwritten: a game whose new folder already has files keeps its place. Close a game before moving it.'))}</p>
      <div class="fs-plans">${list || `<p class="muted">${esc(t('No games are installed.'))}</p>`}</div>
      <div class="acts"><button type="button" class="btn" data-x="no">${esc(t('Cancel'))}</button><button type="button" class="btn main" data-x="go"${n ? '' : ' disabled'}>${esc(tn(n, 'Move 1 Game', 'Move {n} Games'))}</button></div>`;
  };
  const scrim = modal(draw(), async (x, close) => {
    if (x.dataset.x !== 'go' && x.dataset.x !== 'sure') return;
    const box = x.closest('.modal');
    const ids = rows.filter((r) => movable(r) && pick.has(r.id)).map((r) => r.id);
    if (x.dataset.x === 'go') {
      box.innerHTML = `<h2>${esc(tn(ids.length, 'Move 1 game folder?', 'Move {n} game folders?'))}</h2><p>${esc(t('These games can\'t be played until their folders have moved. A move to another drive copies everything first and removes the old folder only once the copy is whole. You can undo it afterwards.'))}</p>
        <div class="acts"><button type="button" class="btn" data-x="no">${esc(t('Cancel'))}</button><button type="button" class="btn main" data-x="sure">${esc(t('Move'))}</button></div>`;
      return;
    }
    scrim.dataset.busy = '1';
    box.innerHTML = `<h2>${esc(t('Moving…'))}</h2><p id="fs-moving">${esc(t('Starting…'))}</p>`;
    let res = [];
    try { res = await invoke('folders_apply', { ids }); } catch (e) { fail(e); }
    delete scrim.dataset.busy;
    await refreshState().catch(() => {});
    FS.data = null;
    await fsLoad();
    const ok = res.filter((r) => r.ok).length;
    box.innerHTML = `<h2>${esc(tn(ok, '1 game moved', '{n} games moved'))}</h2>
      <div class="fs-plans">${res.map((r) => `<div class="fs-plan"><b>${esc(r.name)}</b> <span class="fs-tag ${r.ok ? 'move' : 'blocked'}">${esc(r.ok ? t('Moved') : t('Not moved'))}</span><div class="fs-paths"><code>${esc(r.to)}</code></div>${r.note ? `<p class="fs-note">${esc(t(r.note))}</p>` : ''}</div>`).join('')}</div>
      <div class="acts">${ok ? `<button type="button" class="btn" data-x="undo">${esc(t('Undo'))}</button>` : ''}<button type="button" class="btn main" data-x="no">${esc(t('Done'))}</button></div>`;
    box.addEventListener('click', (e) => { if (e.target.closest('[data-x="undo"]')) { close(); fsUndo(); } }, { once: true });
  }, t('Reorganize Existing Folders'));
  scrim.addEventListener('change', (e) => {
    const c = e.target.closest('input[type="checkbox"][data-id]');
    if (!c) return;
    if (c.checked) pick.add(c.dataset.id); else pick.delete(c.dataset.id);
    const go = scrim.querySelector('[data-x="go"]');
    const n = rows.filter((r) => movable(r) && pick.has(r.id)).length;
    if (go) { go.disabled = !n; go.textContent = tn(n, 'Move 1 Game', 'Move {n} Games'); }
  });
  scrim.querySelector('.modal').classList.add('fs-modal');
}

async function fsUndo() {
  let res;
  try { res = await invoke('folders_undo'); } catch (e) { return fail(e); }
  await refreshState().catch(() => {});
  FS.data = null;
  await fsLoad();
  const ok = res.filter((r) => r.ok).length, bad = res.length - ok;
  toast(bad ? tf('{ok} games went back; {bad} couldn\'t (their old folder has files in it now).', { ok, bad }) : tn(ok, '1 game went back to its old folder.', '{n} games went back to their old folders.'), bad > 0);
}

T.event.listen('folders-move', ({ payload: p }) => {
  const el = document.getElementById('fs-moving');
  if (!el || !p) return;
  const a = (S.library || {})[p.id];
  const name = a ? a.name : p.id;
  el.textContent = p.finished ? tf('{name} moved.', { name }) : p.total ? tf('{name}: {pct}%', { name, pct: Math.floor((100 * p.done) / p.total) }) : name;
});
