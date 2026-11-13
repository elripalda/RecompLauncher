// How engine games look in the launcher, for the desktop and Big Picture alike. Loaded before app.js
// and kept apart from it because that file changes constantly: it calls these with one line each.

// A running engine step, in the shape setupProg gives. Unmeasured work has no percentage to show.
function engineSetupProg(ev) {
  const label = ev.engine_progress?.text || ev.text || t('Setting up');
  return { label, pct: engineSetupPercent(ev.engine_progress) ?? 0 };
}
// An unfinished engine game's rows on Manage. Launch options can't apply: a prepared game starts
// its recorded program, and arguments could select different compiled data.
function engineStartRows(id, inst, rows) {
  if (inst.engine_build?.state !== 'pending') return rows;
  const picked = Object.values(inst.setup_inputs || {})[0];
  const copy = typeof picked === 'string' ? picked : t('Easy Setup builds the game from it.');
  rows.push(`<div class="set-row"><div class="k"><b>${esc(t('Your Copy'))}</b><span>${esc(copy)}</span></div><div class="acts"><button type="button" class="btn" data-act="setup" data-id="${esc(id)}">${esc(picked ? t('Change') : t('Add'))}</button></div></div>`);
  return rows;
}
function engineSetupPercent(p) {
  const fraction = p?.fraction;
  return Number.isFinite(fraction) && fraction >= 0 && fraction <= 1 ? Math.floor(fraction * 100) : null;
}
function setupProgressLabel(ev) {
  const text = ev.engine_progress?.text || ev.text || t('Setting up');
  const pct = engineSetupPercent(ev.engine_progress);
  return pct === null ? text : `${text} · ${ev.engine_progress.estimated ? '~' : ''}${pct}%`;
}
// Shared by desktop and Big Picture. Stage elapsed time and step ETA describe
// different parts of the build, so neither predicts the complete installation.
function engineSetupProgress(p = {}) {
  const valid = (v) => Number.isFinite(v) && v >= 0;
  const pct = engineSetupPercent(p);
  const measured = pct !== null;
  const duration = (seconds) => {
    const s = Math.ceil(seconds), h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60);
    if (h) return tf('{h} h {m} min', { h, m });
    if (m) return tf('{m} min {s} s', { m, s: s % 60 });
    return tf('{s} s', { s });
  };
  const stage = p.stage > 0 && p.stages >= p.stage ? tf('Stage {stage} of {stages}', { stage: p.stage, stages: p.stages }) : '';
  const amount = (n) => p.unit === 'B' ? (n ? fmtB(n) : tf('{n} B', { n: 0 })) : fmtNum(n);
  const count = valid(p.done) && p.total > 0 ? tf('{done} of {total}', { done: amount(p.done), total: (p.estimated ? '~' : '') + amount(p.total) }) + (p.unit && p.unit !== 'B' ? ' ' + p.unit : '') : '';
  const elapsed = valid(p.elapsed_seconds) ? tf('Stage elapsed: {time}', { time: duration(p.elapsed_seconds) }) : '';
  const eta = valid(p.eta_seconds) && p.eta_seconds > 0 ? tf('About {time} remaining for this step', { time: duration(p.eta_seconds) }) : '';
  return `<div class="engine-progress">${stage ? `<p class="muted">${esc(stage)}</p>` : ''}<h3>${esc(p.text || t('Preparing game'))}</h3><p>${esc([count, measured ? (p.estimated ? '~' : '') + pct + '%' : ''].filter(Boolean).join(' · '))}</p><progress max="100"${measured ? ` value="${pct}"` : ''} aria-label="${esc(t('Current step progress'))}"></progress><p class="muted">${esc([elapsed, eta].filter(Boolean).join(' · '))}</p></div>`;
}
function installedVersion(g) {
  return g.engine_build?.game?.version || g.tag || (g.installer_setup && !('engine_build' in g) ? t('Not set up yet') : t('Not known'));
}
function engineVersionCard(a, showDetails = false) {
  const pin = S.library[a.id]?.engine_build?.package;
  const hasRecord = 'engine_build' in (S.library[a.id] || {});
  const checkpoint = typeof pin?.upstream_revision === 'string' ? pin.upstream_revision.slice(0, 12) : '';
  const engine = typeof pin?.engine === 'string' ? pin.engine : '';
  const savedVersion = [engine, checkpoint].filter(Boolean).join(' · ');
  const version = hasRecord ? savedVersion || t('Not known') : a.engine_version || a.engine || '';
  const revision = typeof pin?.packaging_revision === 'string' ? esc(pin.packaging_revision) : '';
  const details = revision ? (showDetails ? `<p>${revision}</p>` : `<details><summary>${esc(t('Build details'))}</summary><p>${revision}</p></details>`) : '';
  return `<div class="stack"><div class="card"><h3>${esc(t('Versions'))}</h3><p>${esc(version)}</p>${details}<p>${esc(t('This game keeps its prepared version. Component downloads and catalog refreshes never rebuild it.'))}</p><p class="muted">${esc(t('Applying game fixes is not available in this test build.'))}</p></div></div>`;
}
