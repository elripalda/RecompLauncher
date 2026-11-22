// Finishing an update that needs a new build: a game set up on an older release builds again (the
// setup status's `changed`), so it's built again before it plays. Play stays
// Play: its first press opens this small window instead of Easy Setup, which is for setting a game
// up, not for updating one that's set up. It runs the same setup as Easy Setup (suBegin and the
// `setup` events in app.js) and only draws it smaller: the build, then the game starts.
//
// SU.upd marks Easy Setup's state as this window's. Stages: 'confirm' (what happens, any option the
// update added, Build Now), then Easy Setup's own 'run', 'error' and 'ready'.

// The words for what it's built from: what the setup step kept (prepare.keeps_copy), or the copy.
function updLead(a, su, g) {
  const ver = verText(g.tag);
  const needs = tf('{name} {version} needs a new build of the game.', { name: a.name, version: ver });
  return `${needs} ${su.kept ? t('Your copy isn\'t needed: it\'s built again from what it kept last time.') : t('It\'s built again from your copy.')}`;
}

// The options its update added (Setup Options' new ones), answered here, as their developer set them.
function updOptRows(id, su) {
  return newPrepOpts(su).map((o) => {
    const on = !!SU.updOpts[o.id];
    const link = o.license && o.license.url ? `<a href="#" data-url="${esc(o.license.url)}">${esc(o.license.name || o.license.url)}</a>` : '';
    const sub = [o.note ? esc(o.note) : '', link ? htmlFill(t('Turning it on accepts the {license}.'), { license: link }) : ''].filter(Boolean).join(' ');
    return `<div class="upd-opt"><div class="k"><b>${esc(o.label)}</b><span class="upd-new">${esc(t('New'))}</span>${sub ? `<span class="sub">${sub}</span>` : ''}</div>`
      + `<button type="button" class="switch" role="switch" aria-checked="${on}" aria-label="${esc(o.label)}" data-up="opt" data-opt="${esc(o.id)}"></button></div>`;
  }).join('');
}

// The run, as Easy Setup counts it (the same steps and weights), with only the steps it has.
function updRun(a, su) {
  const order = (SU.kept ? [] : ['read', 'verify']).concat(SU.extras ? ['extras'] : [], ['setup']);
  const st = SU.steps;
  // (the setup events' "extras" is the game's own setup step, which builds it; "setup" the launcher's
  // last few seconds after it)
  const names = { read: t('Reading your copy'), verify: t('Checking your copy'), extras: t('Building the game'), setup: t('Setting it up') };
  const sub = {
    read: st.read && st.read.text ? st.read.text : baseName(SU.path),
    verify: verifyLine(st.verify, ''),
    // The builder's own line (Compiling, 112 of 611 files changed): what it's doing right now.
    extras: (st.extras && st.extras.text) || '',
    setup: (st.setup && st.setup.text) || '',
  };
  const weight = { read: 5, verify: 40, extras: 50, setup: 5 };
  let total = 0, have = 0;
  const cur = order.indexOf(SU.step);
  for (const k of order) {
    total += weight[k];
    const s = st[k], i = order.indexOf(k);
    have += i < cur || (s && s.done) ? weight[k] : i === cur ? weight[k] * Math.min(1, (s && s.pct) || 0) : 0;
  }
  const pc = Math.round((have / total) * 100);
  const rows = order.map((k, i) => {
    const done = i < cur, now = i === cur || (cur < 0 && i === 0);
    const icon = done ? `<span class="ok">${CHECK}</span>` : now ? '<span class="spinr"></span>' : '<span class="todo"></span>';
    return `<div class="step${!done && !now ? ' wait' : ''}">${icon}<div class="k"><b>${esc(names[k])}</b><span>${esc(sub[k] || (!done && !now ? t('Next') : ''))}</span></div></div>`;
  }).join('');
  return `<div class="steps">${rows}</div><div class="su-bar"><i style="width:${pc}%"></i></div><div class="upd-prog"><span>${pc}%</span></div>`;
}

function updDraw() {
  if (!SU.el || !SU.upd) return;
  const id = SU.id, box = $('.modal', SU.el);
  const a = byId(id) || { name: (S.library[id] || {}).name || id };
  const g = S.library[id] || {};
  const su = S.setup[id] || {};
  // Built: the game starts, as Play asked.
  if (SU.stage === 'ready') { closeSetup(true); return playGame(id); }
  const busy = SU.stage === 'run';
  const ver = busy || !g.prev_tag ? verText(g.tag) : `${verText(g.prev_tag)} → ${verText(g.tag)}`;
  const head = `<button type="button" class="su-x" data-up="close" aria-label="${esc(busy ? t('Hide') : t('Close'))}">${XMARK}</button>
    <div class="upd-head"><div class="upd-art">${gameMediaHtml(a, busy ? 'is-busy' : '')}</div><div><span class="kick">${esc(t('Update'))} · ${esc(a.name)}</span>`
    + `<h2>${esc(busy ? tf('Updating {name}', { name: a.name }) : SU.stage === 'error' ? t('The Update Didn\'t Finish') : t('Finish the Update'))}</h2><span class="upd-ver">${esc(ver)}</span></div></div>`;
  let body = '', foot = '';
  if (SU.stage === 'confirm') {
    const opts = updOptRows(id, su);
    const blocked = prepBlocked(su);
    body = `<p>${esc(updLead(a, su, g))}${opts ? ` ${esc(t('Your other choices stay as they were.'))}` : ''}</p>`
      + (opts || `<div class="upd-note">${esc(t('Your saves and settings stay as they are.'))}</div>`);
    foot = `<div class="upd-foot"><span class="why">${blocked ? esc(prepReq()) : `${CHECK}${esc(t('Ready to build.'))}`}</span>`
      + `<button type="button" class="btn main big" data-up="build"${blocked ? ' disabled' : ''}>${esc(t('Build Now'))}</button></div>`;
  } else if (busy) {
    body = updRun(a, su);
    foot = `<div class="upd-foot"><span class="why">${esc(t('The game starts when it\'s done.'))}</span><button type="button" class="btn" data-up="close">${esc(t('Hide'))}</button></div>`;
  } else {
    body = `<p class="su-err" role="alert">${esc(t(SU.err || 'Setup didn\'t finish.'))}</p>`;
    foot = `<div class="upd-foot"><span class="why"></span><button type="button" class="btn main big" data-up="again">${esc(t('Try Again'))}</button></div>`;
  }
  box.innerHTML = head + body + foot;
}

async function updAct(b) {
  const id = SU.id, su = S.setup[id] || {};
  switch (b.dataset.up) {
    case 'close': return closeSetup();
    case 'again': SU.stage = 'confirm'; SU.err = ''; return updDraw();
    case 'opt': {
      const o = (su.prepare_options || []).find((x) => x.id === b.dataset.opt);
      if (!o) return;
      SU.updOpts[o.id] = !SU.updOpts[o.id];
      o.on = SU.updOpts[o.id];
      return updDraw();
    }
    case 'build': {
      if (prepBlocked(su)) return;
      // The options it added are answered now (as the switches show), so Play doesn't ask again.
      const answers = Object.fromEntries(newPrepOpts(su).map((o) => [o.id, !!SU.updOpts[o.id]]));
      if (Object.keys(answers).length) {
        SU.optsSaving = invoke('set_prepare_opts', { id, opts: answers }).catch(fail);
        for (const o of su.prepare_options || []) if (o.id in answers) { o.on = answers[o.id]; o.new = false; }
      }
      // Built from what it kept (no copy to read or check), or from the copy it was set up with.
      return suBegin(su.kept ? '' : (su.copy || ''), true);
    }
  }
}

// Play's first press after such an update: this window, on the same setup as Easy Setup.
async function updateBuildModal(id) {
  if (document.body.classList.contains('tv-on') && window.tvUpdateBuild) return window.tvUpdateBuild(id);
  if (SU.el && SU.id === id) return;
  if (SU.el && SU.stage === 'run') return toast(t('Finish the setup that\'s running first.'));
  let su = S.setup[id];
  if (su === undefined || su === 'loading') {
    try { su = S.setup[id] = await invoke('game_setup', { id }); } catch (e) { return fail(e); }
  }
  closeSetup(true);
  // The options its update added start as their developer set them (on, unless they say otherwise
  // or have a license to accept), as in Easy Setup.
  const updOpts = Object.fromEntries(newPrepOpts(su).map((o) => [o.id, !!o.on]));
  Object.assign(SU, { id, upd: true, updOpts, stage: 'confirm', named: {}, step: '', ev: {}, steps: {}, q: [], at: 0, path: '', kept: !!su.kept, extras: false, result: null, err: '', note: '' });
  // Its build still running in the background (it was hidden): that build, not a second one.
  const job = S.setupJobs && S.setupJobs[id];
  if (job) {
    const step = job.step || 'setup';
    Object.assign(SU, { stage: 'run', step, steps: { [step]: Object.assign({}, job) }, ev: job, at: Date.now(), extras: !!job.extras, path: job.text || '' });
  }
  const s = document.createElement('div');
  s.className = 'scrim';
  s.innerHTML = `<div class="modal upd" role="dialog" aria-modal="true" aria-label="${esc(t('Finish the Update'))}"></div>`;
  document.body.append(s);
  SU.el = s;
  setupOpened(id);
  s.addEventListener('click', (e) => {
    // A click beside it hides it while it builds (it goes on), and closes it before then.
    if (e.target === s) return closeSetup();
    const link = e.target.closest('a[data-url]');
    if (link) { e.preventDefault(); return invoke('open_url', { url: link.dataset.url }).catch(fail); }
    const b = e.target.closest('[data-up]');
    if (b) updAct(b);
  });
  s.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') { e.preventDefault(); closeSetup(); }
  });
  updDraw();
  const first = $('[data-up="build"], .su-x', s);
  if (first) first.focus();
}
window.updateBuildModal = updateBuildModal;
