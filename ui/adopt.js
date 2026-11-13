'use strict';
// Recomps the player already has (added to Steam, installed by another launcher, or in a folder they
// pick): matched to their recomp, added to the Library where they are, or moved into the Recomp
// Library. From then on they update like the rest.

const ADOPT = { list: [], move: false, busy: false, el: null };
const FROM_WORD = { steam: N_('In Steam'), launcher: N_('From another launcher'), folder: N_('A folder you picked'), linked: N_('Found its game: Play starts it from here now') };

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
  const sameFolder = Object.values(S.library).some((g) => adoptKey(g) === adoptKey(f));
  const pickOne = f.from !== 'linked' && !sameFolder
    ? `<button type="button" class="btn sm" data-x="project" data-i="${i}">${esc(t(f.id || f.any ? 'Change…' : 'Choose project…'))}</button>`
    : '';
  return `<div class="adopt-row${f.have ? ' have' : ''}"><label class="adopt-l">
    <input type="checkbox" data-i="${i}"${f.on ? ' checked' : ''}${f.have || !(f.id || f.any) ? ' disabled' : ''}>
    ${(a && a.cover && pic(a.cover)) || '<span class="ph"></span>'}
    <span class="adopt-k"><b>${esc(adoptName(f))}</b><small class="path">${esc(f.path)}</small>
      <small>${esc(t(FROM_WORD[f.from] || FROM_WORD.folder))} · ${esc(f.have ? t('Already in your Library') : f.any ? t('Untracked · no updates') : ver)}${a && a.repo ? ' · ' + esc(a.repo) : ''}</small></span></label>
    ${pickOne}</div>`;
}
function adoptDraw() {
  if (!ADOPT.el || !ADOPT.el.isConnected) return;
  const box = $('.modal', ADOPT.el);
  const list = ADOPT.list;
  const n = list.filter((f) => f.on && (f.id || f.any) && !f.have).length;
  const other = list.some((f) => f.on && f.from === 'launcher');
  const body = ADOPT.busy === 'scan'
    ? `<p class="muted adopt-wait">${esc(t('Looking in other launchers, Steam and the usual folders…'))}</p>`
    : list.length
      ? `<div class="adopt-list">${list.map(adoptRow).join('')}</div>`
      : `<p class="set-empty">${esc(t('None found in other launchers or Steam. If yours are somewhere else, pick the folder.'))}</p>`;
  box.innerHTML = `<h2>${esc(t('Recomps you already have'))}</h2>
    <p>${esc(t('Check the suggested project for each installation. Tracked projects get updates here; untracked installations launch without update checks. Your folders stay where they are.'))}</p>
    ${body}
    ${other ? `<div class="set-row adopt-move"><div class="k"><b>${esc(t('Move the Ones From Another Launcher Into Your Recomp Library'))}</b><span>${esc(tf('Into {dir}. The other launcher won\'t see them after that.', { dir: S.settings.library_dir }))}</span></div><button type="button" class="switch" role="switch" aria-checked="${ADOPT.move}" aria-label="${esc(t('Move them'))}" data-x="move"></button></div>` : ''}
    <div class="acts" style="justify-content:space-between"><button type="button" class="btn" data-x="pick"${ADOPT.busy ? ' disabled' : ''}>${esc(t('Pick a folder…'))}</button>
      <span style="display:flex;gap:10px"><button type="button" class="btn" data-x="no">${esc(t('Cancel'))}</button><button type="button" class="btn main" data-x="yes"${n && !ADOPT.busy ? '' : ' disabled'}>${esc(ADOPT.busy === 'add' ? t('Adding…') : n ? tn(n, 'Add 1 game', 'Add {n} games') : t('Add'))}</button></span></div>`;
}

async function adoptOpen(found) {
  ADOPT.list = []; ADOPT.move = false; ADOPT.busy = found ? false : 'scan';
  if (found) adoptMerge(found);
  ADOPT.el = modal('<div></div>', async (x, close) => {
    const w = x.dataset.x;
    if (w === 'project') {
      const f = ADOPT.list[Number(x.dataset.i)];
      return projectPicker(f.id || '', '', async (project) => {
        if (project !== f.id) f.tag = '';
        f.id = project;
        f.any = !project;
        f.on = true;
        f.have = !!(project && S.library[project]);
        adoptDraw();
      });
    }
    if (w === 'move') { ADOPT.move = !ADOPT.move; return adoptDraw(); }
    if (w === 'pick') {
      const p = await pickFile({ title: t('A game\'s folder, or another launcher\'s folder'), directory: true, multiple: false }).catch(fail);
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
    adoptDraw();
  });
  adoptDraw();
  if (!found) {
    try { adoptMerge(await invoke('adopt_scan')); } catch (e) { fail(e); }
    ADOPT.busy = false;
    adoptDraw();
  }
}

// Shared by imports and Manage: select the exact project, not just a game's title.
function projectPicker(current, installedId, save) {
  const projects = all().filter((a) => a.repo && !a.soon).sort((a, b) => a.name.localeCompare(b.name) || a.repo.localeCompare(b.repo));
  let selected = current || '';
  const m = modal(`<h2>${esc(t('Project association'))}</h2>
    <p>${esc(t('Choose the project this installation belongs to, or keep it untracked. Changing this does not download or replace any game files.'))}</p>
    <label class="mh-f"><span>${esc(t('Search projects'))}</span><input class="field" id="project-search" type="search" autocomplete="off"></label>
    <label class="mh-f"><span>${esc(t('Project'))}</span><select class="field project-options" id="project-choice" size="8"></select></label>
    <p class="muted" id="project-note"></p>
    <p class="err" id="project-error" hidden></p>
    <div class="acts"><button type="button" class="btn" data-x="no">${esc(t('Cancel'))}</button><button type="button" class="btn main" data-x="yes">${esc(t('Save'))}</button></div>`, async (x, close) => {
      if (x.dataset.x !== 'yes' || m.dataset.busy) return;
      m.dataset.busy = '1';
      $('[data-x="yes"]', m).disabled = true;
      $('#project-choice', m).disabled = true;
      $('#project-search', m).disabled = true;
      try { await save(selected); close(); }
      catch (e) {
        const err = $('#project-error', m); err.textContent = String(e); err.hidden = false;
        delete m.dataset.busy;
        $('#project-choice', m).disabled = false;
        $('#project-search', m).disabled = false;
        update();
      }
    }, t('Project association'));
  $('.modal', m).classList.add('project-picker');
  const update = () => {
    const conflict = selected && selected !== installedId && S.library[selected];
    $('[data-x="yes"]', m).disabled = !!conflict;
    $('#project-note', m).textContent = conflict ? t('That project is already in your Library. Choose another project or keep this installation untracked.')
      : selected && selected === installedId ? t('This is the current project. Choose another project to correct the match.')
      : selected ? t('Updates will be checked against this project. Automatic updates stay off until you enable them.')
      : t('Keep untracked: launch this installation without checking a project’s releases for updates.');
  };
  const draw = () => {
    const q = $('#project-search', m).value.trim().toLowerCase();
    const list = projects.filter((a) => a.id === selected || `${a.name} ${a.repo}`.toLowerCase().includes(q));
    $('#project-choice', m).innerHTML = `<option value="">${esc(t('Keep untracked'))}</option>` + list.map((a) => `<option value="${esc(a.id)}">${esc(a.name)} — ${esc(a.repo)}</option>`).join('');
    $('#project-choice', m).value = selected;
    update();
  };
  $('#project-search', m).addEventListener('input', draw);
  $('#project-choice', m).addEventListener('change', (e) => { selected = e.target.value; update(); });
  draw();
  $('#project-search', m).focus();
  return m;
}

function changeProject(id) {
  const g = S.library[id];
  if (!g) return;
  if (S.running.has(id) || S.jobs[id]) return toast(t('Close the game and wait for its downloads first.'), true);
  return projectPicker(g.local ? '' : id, id, async (project) => {
    // Invalidate any update check started against the previous association.
    updateCheckRevision++;
    const next = await invoke('set_project', { id, project });
    updateCheckRevision++;
    delete S.updates[id]; delete S.updates[next.id];
    for (const cache of [S.setup, S.mods, S.modList, S.info, S.builds, S.reports]) {
      delete cache[id]; delete cache[next.id];
    }
    await refreshState();
    if (S.arg === id) S.arg = next.id;
    render(true); renderSide();
    if (window.tvRefresh) window.tvRefresh();
    toast(t('Project association saved. Your game files were not changed.'));
  });
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
  const moving = ADOPT.move ? pick.filter((f) => f.from === 'launcher' && added.some((g) => g.id === f.id)) : [];
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

// Once, a few seconds after start: if Steam or another launcher has recomps that aren't in the
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

// ---------- Settings → Bring Your Games ----------
// Everything from another launcher, Steam or a folder, in one place. Games it's sure about (their
// project is the recomp's) come straight in; the player decides about the rest, one at a time, and
// nothing unsure is added without them. Games only in the other launcher's list come over too:
// saved to the Library, or added from their project's link. Ones hidden there stay hidden here.
const MIG = { busy: '', list: null, off: new Set(), note: '' };

// A game added from its link earlier: its project is already one of the player's own.
function migKnownLink(f) {
  const r = String(f.link || '').toLowerCase().replace(/^https?:\/\/(www\.)?/, '').replace(/\/+$/, '');
  const p = r.startsWith('gitlab.com/') ? 'gitlab:' + r.slice(11) : r.replace(/^github\.com\//, '');
  return !!p && all().some((a) => String(a.repo || '').toLowerCase() === p);
}
function migGroups() {
  const live = (MIG.list || []).filter((f) => !f.have);
  return {
    ready: live.filter((f) => !f.missing && f.sure && f.id),
    look: live.filter((f) => !f.missing && !(f.sure && f.id)),
    away: live.filter((f) => f.missing && (f.sure && f.id ? !S.library[f.id] && !isSaved(f.id) : !!f.link && !migKnownLink(f))),
    have: (MIG.list || []).filter((f) => f.have).length,
  };
}
function migMerge(list) {
  MIG.list = MIG.list || [];
  for (const f of list) {
    const i = MIG.list.findIndex((x) => adoptKey(x) === adoptKey(f));
    if (i >= 0) MIG.list[i] = Object.assign(MIG.list[i], f); else MIG.list.push(f);
  }
}
function migRedraw() { if (S.view === 'settings' && S.setTab === 'bring') setSection('bring', false); }
async function migScan() {
  MIG.busy = 'scan'; MIG.note = ''; migRedraw();
  try { const got = await invoke('migrate_scan'); MIG.list = null; migMerge(got); } catch (e) { fail(e); }
  MIG.busy = ''; migRedraw();
}
// Games hidden in the other launcher: hidden from Discover and Home here too.
async function migHide(ids) {
  const add = ids.filter(Boolean);
  if (add.length) await saveUi({ hidden: [...new Set([...hiddenSet(), ...add])] }, true);
}
// One game into the Library where it is: `id` a recomp, or empty for one of the player's own.
async function migAdopt(f, id, fromLink) {
  const tag = (id && f.sure && id === f.id) || fromLink ? f.tag || '' : '';
  const got = await invoke('adopt_add', { items: [{ id: id || '', path: f.path, exe: f.exe || null, tag }] });
  f.have = true;
  if (f.hidden && id) await migHide([id]);
  for (const g of got) invoke('usage_event', { kind: 'setup', game: g.id, version: g.tag || null, detail: 'brought over' }).catch(() => {});
  return got.length;
}
async function migDone(n) {
  await refreshState();
  checkUpdates().then(() => render(true));
  if (n) toast(tn(n, '1 game is in your Library now.', '{n} games are in your Library now.'));
  render(true); renderSide();
  if (window.tvRefresh) window.tvRefresh();
}

function migratePage(group) {
  const g = MIG.list ? migGroups() : null;
  const busy = !!MIG.busy;
  let body = `<p class="set-note">${esc(t('Coming from another launcher? Bring your games over. The ones it\'s sure about come straight in, and you decide on the rest. Your game folders stay where they are.'))}</p>
    <div class="mig-acts"><button type="button" class="btn sm main" data-act="mig-scan"${busy ? ' disabled' : ''}>${esc(MIG.list ? t('Look Again') : t('Look for My Games'))}</button><button type="button" class="btn sm" data-act="mig-pick"${busy ? ' disabled' : ''}>${esc(t('Pick a Folder…'))}</button></div>`;
  if (MIG.busy === 'scan') body += `<p class="muted mig-wait">${esc(t('Looking in other launchers, Steam and the usual folders…'))}</p>`;
  else if (g) {
    const box = (n, title, sub, act, label) => `<div class="mig-box"><b>${n}</b><span>${esc(title)}</span><small>${esc(sub)}</small>
      <button type="button" class="btn sm${act === 'mig-add' ? ' main' : ''}" data-act="${act}"${n && !busy ? '' : ' disabled'}>${esc(label)}</button></div>`;
    const ready = g.ready.filter((f) => !MIG.off.has(adoptKey(f)));
    if (!g.ready.length && !g.look.length && !g.away.length) {
      body += `<p class="set-empty">${esc(g.have ? t('Everything it found is already in your Library.') : t('No games found. If yours are somewhere else, pick their folder or the other launcher\'s folder.'))}</p>`;
    } else {
      body += `<div class="mig-sum">
        ${box(ready.length, t('Ready to Add'), t('It knows which games these are'), 'mig-add', t('Add Them'))}
        ${box(g.look.length, t('Need a Look'), t('You pick which game each one is'), 'mig-look', t('Go Through Them'))}
        ${box(g.away.length, t('Not on This PC'), t('Only in the other launcher\'s list'), 'mig-away', t('Add to Library'))}
      </div>`;
      if (MIG.note) body += `<p class="muted mig-wait">${esc(MIG.note)}</p>`;
      if (g.ready.length) {
        body += `<div class="mig-list">${g.ready.map((f) => { const a = byId(f.id); const k = adoptKey(f); return `<label class="mig-row">
          <input type="checkbox" data-act="mig-tick" data-k="${esc(k)}"${MIG.off.has(k) ? '' : ' checked'}${busy ? ' disabled' : ''}>
          ${(a && a.cover && pic(a.cover)) || '<span class="ph"></span>'}
          <span class="adopt-k"><b>${esc(a ? a.name : f.name)}</b><small class="path">${esc(f.path)}</small><small>${esc(t(FROM_WORD[f.from] || FROM_WORD.folder))}${f.tag ? ' · ' + esc(f.tag) : ''}${f.hidden ? ' · ' + esc(t('Hidden there, hidden here too')) : ''}</small></span></label>`; }).join('')}</div>`;
      }
    }
    if (g.have) body += `<p class="set-foot">${esc(tn(g.have, '1 game it found is already in your Library.', '{n} games it found are already in your Library.'))}</p>`;
  }
  return group(t('Bring Your Games'), body);
}

// Need a Look: one game at a time. The closest recomps first; or search them all, add it from its
// project's link, or add it without updates. Each one is added only once the player says which.
function migLook() {
  const list = migGroups().look;
  if (!list.length) return;
  let i = 0, added = 0;
  const near = (f) => {
    const ids = [...(f.guesses || [])];
    if (f.id && !ids.includes(f.id)) ids.unshift(f.id);
    return ids.map(byId).filter((a) => a && !S.library[a.id]);
  };
  const draw = () => {
    const f = list[i], opts = near(f);
    return `<h2>${esc(t('Which Game Is This?'))}</h2>
      <p class="mig-what"><b>${esc(f.name)}</b><small class="path">${esc(f.path)}</small>${f.link ? `<small>${esc(f.link)}</small>` : ''}</p>
      ${opts.length ? `<p>${esc(t('These look closest. Pick the one it is.'))}</p><div class="choices">${opts.map((a) => `<button type="button" class="choice" data-x="pick" data-id="${esc(a.id)}"><span><b>${esc(a.name)}</b><small>${esc(a.repo || '')}</small></span></button>`).join('')}</div>`
        : `<p class="muted">${esc(t('No recomp in RecompLauncher has a name like this one.'))}</p>`}
      <div class="acts mig-look-acts">
        <button type="button" class="btn" data-x="search">${esc(t('Search All Recomps…'))}</button>
        ${f.link ? `<button type="button" class="btn" data-x="link">${esc(t('Add From Its Project'))}</button>` : ''}
        <button type="button" class="btn" data-x="own">${esc(t('Add Without Updates'))}</button>
        <button type="button" class="btn" data-x="skip">${esc(t('Skip'))}</button>
      </div>
      <p class="muted mig-count">${esc(tf('{a} of {b}', { a: i + 1, b: list.length }))}</p>`;
  };
  const m = modal(draw(), async (x, close) => {
    const w = x.dataset.x, f = list[i];
    if (!w || w === 'no' || m.dataset.busy) return;
    const next = async () => {
      i++;
      if (i >= list.length) { close(); migRedraw(); return migDone(added); }
      $('.modal', m).innerHTML = draw();
    };
    if (w === 'skip') return next();
    if (w === 'search') {
      return projectPicker(f.id || '', '', async (project) => { added += await migAdopt(f, project); await next(); });
    }
    m.dataset.busy = '1';
    try {
      if (w === 'pick') added += await migAdopt(f, x.dataset.id);
      else if (w === 'own') added += await migAdopt(f, '');
      else if (w === 'link') { const r = await invoke('add_github', { link: f.link }); await refreshState(); added += await migAdopt(f, r.id, !r.known); }
      delete m.dataset.busy;
      await next();
    } catch (e) { delete m.dataset.busy; fail(e); }
  }, t('Which Game Is This?'));
  $('.modal', m).classList.add('mig-look');
}

// Not on This PC: ones RecompLauncher has are saved to the Library to install whenever; the rest are
// added from their project's link, one by one (GitHub may ask for a break on a long list).
async function migAway() {
  const list = migGroups().away;
  if (!list.length) return;
  MIG.busy = 'away';
  const keep = list.filter((f) => f.sure && f.id), links = list.filter((f) => !(f.sure && f.id));
  let n = 0;
  if (keep.length) {
    const when = Math.floor(Date.now() / 1000), s = Object.assign({}, saved());
    for (const f of keep) { s[f.id] = s[f.id] || when; f.have = true; }
    await saveUi({ saved: s }, true);
    for (const f of keep) libSend(f.id, true);
    await migHide(keep.filter((f) => f.hidden).map((f) => f.id));
    n += keep.length;
  }
  const hid = [];
  for (let k = 0; k < links.length; k++) {
    MIG.note = tf('Adding {a} of {b} from their projects…', { a: k + 1, b: links.length });
    migRedraw();
    try {
      const r = await invoke('add_github', { link: links[k].link });
      links[k].have = true; n++;
      if (links[k].hidden && r.id) hid.push(r.id);
    } catch (e) {
      MIG.note = tf('Stopped at {name}: {why} Press Add to Library again later for the rest.', { name: links[k].name, why: String(e) });
      break;
    }
    if (k === links.length - 1) MIG.note = '';
  }
  await migHide(hid);
  MIG.busy = '';
  await migDone(0);
  if (n) toast(tn(n, '1 game is in your Library now. Install it whenever you like.', '{n} games are in your Library now. Install them whenever you like.'));
  migRedraw();
}

async function migAddReady() {
  const list = migGroups().ready.filter((f) => !MIG.off.has(adoptKey(f)));
  if (!list.length) return;
  MIG.busy = 'add'; migRedraw();
  let got = [];
  try { got = await invoke('adopt_add', { items: list.map((f) => ({ id: f.id, path: f.path, exe: f.exe || null, tag: f.tag || '' })) }); } catch (e) { MIG.busy = ''; migRedraw(); return fail(e); }
  const ids = new Set(got.map((g) => g.id));
  for (const f of list) if (ids.has(f.id)) f.have = true;
  await migHide(list.filter((f) => f.hidden && ids.has(f.id)).map((f) => f.id));
  for (const g of got) invoke('usage_event', { kind: 'setup', game: g.id, version: g.tag || null, detail: 'brought over' }).catch(() => {});
  MIG.busy = '';
  await migDone(got.length);
  migRedraw();
}

window.migratePage = migratePage;
window.migrateAct = async (act, el) => {
  if (act === 'mig-open') return setSection('bring');
  if (MIG.busy) return;
  if (act === 'mig-scan') return migScan();
  if (act === 'mig-pick') {
    const p = await pickFile({ title: t('A game\'s folder, or another launcher\'s folder'), directory: true, multiple: false }).catch(fail);
    if (!p) return;
    try { migMerge(await invoke('migrate_pick', { path: Array.isArray(p) ? p[0] : p })); } catch (e) { fail(e); }
    return migRedraw();
  }
  if (act === 'mig-tick') { const k = el.dataset.k; if (el.checked) MIG.off.delete(k); else MIG.off.add(k); return migRedraw(); }
  if (act === 'mig-add') return migAddReady();
  if (act === 'mig-look') return migLook();
  if (act === 'mig-away') return migAway();
};
