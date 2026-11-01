'use strict';
// Recomps the player already has (added to Steam, installed by Quiver Launcher, or in a folder they
// pick): matched to their recomp, added to the Library where they are, or moved into the Recomp
// Library. From then on they update like the rest.

const ADOPT = { list: [], move: false, busy: false, el: null };
const FROM_WORD = { steam: N_('In Steam'), quiver: N_('From Quiver Launcher'), folder: N_('A folder you picked'), linked: N_('Found its game: Play starts it from here now') };

function adoptName(f) { const a = f.id && byId(f.id); return a ? a.name : f.name; }
function adoptKey(f) { return String(f.path).toLowerCase(); }
function adoptMerge(list) {
  for (const f of list) {
    const i = ADOPT.list.findIndex((x) => adoptKey(x) === adoptKey(f));
    if (i >= 0) ADOPT.list[i] = Object.assign(ADOPT.list[i], f); else ADOPT.list.push(Object.assign({ on: !f.have && !!f.id }, f));
  }
}
function adoptRow(f, i) {
  const a = f.id && byId(f.id);
  const ver = f.tag ? f.tag : t('version not known');
  // Typing filters the list (hundreds of recomps): pick one, or add it as your own.
  const pickOne = !f.id && !f.have
    ? `<input class="field adopt-which" list="adopt-names" data-i="${i}" placeholder="${esc(f.any ? t('Not in the list: add it anyway') : t('Which recomp is this?'))}" aria-label="${esc(t('Which recomp is this?'))}" autocomplete="off" spellcheck="false">`
    : '';
  return `<div class="adopt-row${f.have ? ' have' : ''}"><label class="adopt-l">
    <input type="checkbox" data-i="${i}"${f.on ? ' checked' : ''}${f.have || !(f.id || f.any) ? ' disabled' : ''}>
    ${(a && a.cover && pic(a.cover)) || '<span class="ph"></span>'}
    <span class="adopt-k"><b>${esc(adoptName(f))}</b><small class="path">${esc(f.path)}</small>
      <small>${esc(t(FROM_WORD[f.from] || FROM_WORD.folder))} · ${esc(f.have ? t('Already in your Library') : ver)}</small></span></label>
    ${pickOne}</div>`;
}
function adoptDraw() {
  if (!ADOPT.el || !ADOPT.el.isConnected) return;
  const box = $('.modal', ADOPT.el);
  const list = ADOPT.list;
  const n = list.filter((f) => f.on && (f.id || f.any) && !f.have).length;
  const quiver = list.some((f) => f.on && f.from === 'quiver');
  const body = ADOPT.busy === 'scan'
    ? `<p class="muted adopt-wait">${esc(t('Looking in Steam, Quiver Launcher and the usual folders…'))}</p>`
    : list.length
      ? `<div class="adopt-list">${list.map(adoptRow).join('')}</div><datalist id="adopt-names"><option value="${esc(t('Not in the list: add it anyway'))}"></option>${all().filter(forOS).sort((x, y) => x.name.localeCompare(y.name)).map((x) => `<option value="${esc(x.name)}"></option>`).join('')}</datalist>`
      : `<p class="set-empty">${esc(t('None found in Steam or Quiver Launcher. If yours are somewhere else, pick the folder.'))}</p>`;
  box.innerHTML = `<h2>${esc(t('Recomps you already have'))}</h2>
    <p>${esc(t('They stay where they are and update from here, in the same folder, so Steam shortcuts keep working. Saves are backed up before every update.'))}</p>
    ${body}
    ${quiver ? `<div class="set-row adopt-move"><div class="k"><b>${esc(t('Move the Quiver Launcher ones into your Recomp Library'))}</b><span>${esc(tf('Into {dir}. Quiver Launcher won\'t see them after that.', { dir: S.settings.library_dir }))}</span></div><button type="button" class="switch" role="switch" aria-checked="${ADOPT.move}" aria-label="${esc(t('Move them'))}" data-x="move"></button></div>` : ''}
    <div class="acts" style="justify-content:space-between"><button type="button" class="btn" data-x="pick"${ADOPT.busy ? ' disabled' : ''}>${esc(t('Pick a folder…'))}</button>
      <span style="display:flex;gap:10px"><button type="button" class="btn" data-x="no">${esc(t('Cancel'))}</button><button type="button" class="btn main" data-x="yes"${n && !ADOPT.busy ? '' : ' disabled'}>${esc(ADOPT.busy === 'add' ? t('Adding…') : n ? tn(n, 'Add 1 game', 'Add {n} games') : t('Add'))}</button></span></div>`;
}

async function adoptOpen(found) {
  ADOPT.list = []; ADOPT.move = false; ADOPT.busy = found ? false : 'scan';
  if (found) adoptMerge(found);
  ADOPT.el = modal('<div></div>', async (x, close) => {
    const w = x.dataset.x;
    if (w === 'move') { ADOPT.move = !ADOPT.move; return adoptDraw(); }
    if (w === 'pick') {
      const p = await pickFile({ title: t('The folder the game is in, or Quiver Launcher\'s folder'), directory: true, multiple: false }).catch(fail);
      if (!p) return;
      try { const got = await invoke('adopt_pick', { path: Array.isArray(p) ? p[0] : p }); adoptMerge(got); if (got.some((f) => f.from === 'linked')) refreshState().then(() => render(true)); } catch (e) { fail(e); }
      return adoptDraw();
    }
    if (w === 'yes') return adoptAdd(close);
  }, t('Recomps you already have'));
  $('.modal', ADOPT.el).classList.add('adopt');
  ADOPT.el.addEventListener('change', (e) => {
    const i = Number(e.target.dataset.i);
    const f = ADOPT.list[i];
    if (!f) return;
    if (e.target.matches('input[type=checkbox]')) f.on = e.target.checked;
    if (e.target.matches('.adopt-which')) {
      // Not one the launcher knows: it goes in as the player's own game (plays, but doesn't update).
      const v = e.target.value.trim();
      const hit = all().find((x) => x.name.toLowerCase() === v.toLowerCase());
      f.any = v === t('Not in the list: add it anyway');
      if (!hit && !f.any) return;
      f.id = f.any ? '' : hit.id; f.on = !!(f.id || f.any);
      f.have = !!(f.id && S.library[f.id]);
      if (f.have) toast(tf('{name} is already in your Library.', { name: adoptName(f) }));
    }
    adoptDraw();
  });
  adoptDraw();
  if (!found) {
    try { adoptMerge(await invoke('adopt_scan')); } catch (e) { fail(e); }
    ADOPT.busy = false;
    adoptDraw();
  }
}

async function adoptAdd(close) {
  const pick = ADOPT.list.filter((f) => f.on && (f.id || f.any) && !f.have);
  // The same recomp twice: the first one. The player's own games go by their folder.
  const seen = new Set();
  const items = pick.filter((f) => { const k = f.id || adoptKey(f); return !seen.has(k) && seen.add(k); }).map((f) => ({ id: f.id || '', path: f.path, exe: f.exe || null, tag: f.tag || '' }));
  if (!items.length) return;
  ADOPT.busy = 'add'; adoptDraw();
  let added = [];
  try { added = await invoke('adopt_add', { items }); } catch (e) { ADOPT.busy = false; adoptDraw(); return fail(e); }
  close();
  await saveUi({ adopt_asked: true });
  await refreshState();
  const moving = ADOPT.move ? pick.filter((f) => f.from === 'quiver' && added.some((g) => g.id === f.id)) : [];
  for (const f of moving) {
    toast(tf('Moving {name} into your Recomp Library…', { name: adoptName(f) }));
    try { await invoke('move_game', { id: f.id, to: S.settings.library_dir }); } catch (e) { fail(e); }
  }
  await refreshState();
  checkUpdates().then(() => render(true));
  toast(tn(added.length, '1 game added. If there\'s a newer version, it shows Update.', '{n} games added. The ones with a newer version show Update.'));
  for (const g of added) invoke('usage_event', { kind: 'setup', game: g.id, version: g.tag || null, detail: 'added existing' }).catch(() => {});
  render(true);
}

window.adoptAct = (act) => (act === 'adopt' ? adoptOpen() : null);

// Once, a few seconds after start: if Steam or Quiver Launcher has recomps that aren't in the
// Library yet, say so.
setTimeout(async () => {
  if ((S.settings.ui || {}).adopt_asked || !S.settings.setup_done || document.body.classList.contains('tv-on')) return;
  let found = [];
  try { found = await invoke('adopt_scan'); } catch (e) { return; }
  const fresh = found.filter((f) => f.id && !f.have);
  await saveUi({ adopt_asked: true });
  if (!fresh.length) return;
  toast(tn(fresh.length, 'Found 1 recomp you already have.', 'Found {n} recomps you already have.'), false,
    `${esc(tn(fresh.length, 'Found 1 recomp you already have.', 'Found {n} recomps you already have.'))} <a href="#" data-act="adopt">${esc(t('Add to Library'))}</a>`, 20000);
}, 9000);
