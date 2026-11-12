// Customize: one window for a game's name and its pictures (cover, background, logo, icon). Each
// picture comes from SteamGridDB (with the player's own key) or the player's own file, and is
// framed here: dragged into place and zoomed inside a frame of the shape it's shown in, with
// previews of where it shows. What was picked and how it was framed are kept, so it opens again
// just as it was left.
(function () {
  const KINDS = [
    { k: 'cover', label: N_('Cover'), w: 600, h: 900, fill: true, type: 'image/jpeg', sgdb: 'covers' },
    { k: 'hero', label: N_('Background'), w: 1920, h: 620, fill: true, type: 'image/jpeg', sgdb: 'heroes' },
    { k: 'logo', label: N_('Logo'), w: 1000, h: 500, fill: false, type: 'image/png', sgdb: 'logos' },
    { k: 'icon', label: N_('Icon'), w: 256, h: 256, fill: true, type: 'image/png', sgdb: 'icons' },
  ];
  const kindOf = (k) => KINDS.find((x) => x.k === k);
  const ZMIN = 0.2, ZMAX = 5;

  // Where the picture sits in its frame: scale and top-left corner, for a frame w x h.
  function place(img, spec, st, w, h) {
    const iw = img.naturalWidth || img.width, ih = img.naturalHeight || img.height;
    const fit = Math.min(w / iw, h / ih), fill = Math.max(w / iw, h / ih);
    const s = (spec.fill ? fill : fit) * st.zoom;
    return { s, x: w / 2 + st.x * w - (iw * s) / 2, y: h / 2 + st.y * h - (ih * s) / 2, iw, ih, fit, fill };
  }
  function draw(ctx, img, spec, st, fx, fy, w, h) {
    const p = place(img, spec, st, w, h);
    ctx.imageSmoothingQuality = 'high';
    ctx.drawImage(img, fx + p.x, fy + p.y, p.iw * p.s, p.ih * p.s);
  }
  // The finished picture at its full size, as a data: address.
  function renderArt(spec, ed) {
    const c = document.createElement('canvas');
    c.width = spec.w; c.height = spec.h;
    const ctx = c.getContext('2d');
    if (spec.type === 'image/jpeg') { ctx.fillStyle = '#000'; ctx.fillRect(0, 0, spec.w, spec.h); }
    draw(ctx, ed.img, spec, ed, 0, 0, spec.w, spec.h);
    return c.toDataURL(spec.type, 0.92);
  }
  const loadImg = (src) => new Promise((ok, no) => { const i = new Image(); i.onload = () => ok(i); i.onerror = () => no(new Error(t('That picture couldn\'t be opened.'))); i.src = src; });

  // A picture from this PC for a game, framed the usual way (filling a cover or background, a logo
  // or icon whole) and kept: for Add From GitHub and Edit Game Info, which have no frame of their own.
  window.ownArt = {
    // The picture read in (kept in the covers folder until it's saved or left): { src, data }.
    async open(id, kind, path) { return invoke('art_source', { id: id || 'new', kind, path }); },
    async save(id, kind, pic) {
      const spec = kindOf(kind);
      const st = { img: await loadImg(pic.data), x: 0, y: 0, zoom: 1 };
      const r = await invoke('art_save', { id, kind, data: renderArt(spec, st), src: pic.src, frame: { x: 0, y: 0, zoom: 1 } });
      if (r && r.art_only) { delete r.art_only; if (Object.keys(r).length) S.arts[id] = r; else delete S.arts[id]; } else if (r) S.library[id] = r;
    },
    // Back to the usual picture for that kind.
    async clear(id, kind) {
      const r = await invoke('art_save', { id, kind, data: null, src: null, frame: null });
      if (r && r.art_only) { delete r.art_only; if (Object.keys(r).length) S.arts[id] = r; else delete S.arts[id]; } else if (r) S.library[id] = r;
    },
  };

  window.customizeModal = async function (id) {
    // Any game: an installed one, or one from Discover or the Library before it's installed.
    const g = S.library[id];
    const a = g ? libApp(g) : byId(id);
    if (!a) return;
    const names = Object.assign({}, (S.settings.ui || {}).names);
    const abouts = Object.assign({}, (S.settings.ui || {}).abouts);
    const own = a._name !== undefined ? a._name : a.name;
    // Per picture: { src, img, x, y, zoom, changed, reset }.
    const ed = {};
    let tab = 'cover', browse = null;
    const key = () => ((S.settings.ui || {}).sgdb_key || '').trim();
    // What's shown now for a kind: the framed picture, else the game's own (an address).
    const usual = (k) => ({ cover: a.cover, hero: a.hero, logo: a.logo, icon: a.icon })[k] || '';
    // A game with versions: each tab can go to every version of it, and to versions still to come
    // (kept under the game's key). The tabs ticked for that.
    const group = verGroup(a);
    const every = new Set();
    const m = modal(`<button type="button" class="su-x" data-x="no" aria-label="${esc(t('Close'))}">${XMARK}</button>
      <h2>${esc(tf('Customize {name}', { name: a.name }))}</h2>
      <div class="seg cz-tabs" role="tablist">${[{ k: 'name', label: N_('Info') }, ...KINDS].map((x) => `<button type="button" role="tab" data-x="tab" data-k="${x.k}">${esc(t(x.label))}</button>`).join('')}</div>
      <div class="cz-body" id="cz-body"></div>
      ${group ? `<label class="cz-every"><input type="checkbox" id="cz-every"><span>${esc(tf('Use this for every version of {game}', { game: group.name }))}</span></label>` : ''}
      <div class="acts grid2"><button type="button" class="btn" data-x="no">${esc(t('Cancel'))}</button><button type="button" class="btn main" data-x="save">${esc(t('Save'))}</button></div>`, onClick, t('Customize'));
    const box = $('.modal', m);
    box.classList.add('cz');
    const everyBox = $('#cz-every', box);
    if (everyBox) everyBox.addEventListener('change', () => { if (everyBox.checked) every.add(tab); else every.delete(tab); });

    // The pictures the player framed before open as they were.
    await Promise.all(KINDS.map(async (spec) => {
      try {
        const o = await invoke('art_open', { id, kind: spec.k });
        if (o && o.data) ed[spec.k] = { src: o.src, img: await loadImg(o.data), x: +o.x || 0, y: +o.y || 0, zoom: +o.zoom || 1, changed: false };
      } catch (e) { /* opens with the usual picture */ }
    }));
    show('cover');

    function current(k) {
      const e = ed[k];
      if (e && !e.reset) return { ed: e };
      if (e && e.reset) return { url: usualArt(a, k) };
      return { url: usual(k) };
    }

    function show(k) {
      tab = k;
      browse = null;
      if (everyBox) everyBox.checked = every.has(k);
      for (const b of $$('[data-x="tab"]', box)) { const on = b.dataset.k === k; b.setAttribute('aria-selected', String(on)); b.classList.toggle('on', on); }
      const body = $('#cz-body', box);
      if (k === 'name') {
        body.innerHTML = `<div class="cz-name"><label class="cz-lab" for="cz-n">${esc(t('Name'))}</label>
          <input class="field" id="cz-n" type="text" value="${esc(names[id] || '')}" placeholder="${esc(own)}" spellcheck="false" autocomplete="off" maxlength="80">
          <p class="muted">${esc(tf('Shown everywhere in the launcher, in Steam and on shortcuts instead of "{name}". Leave it empty for its own name.', { name: own }))}</p>
          <label class="cz-lab" for="cz-d">${esc(t('Your Description'))}</label>
          <textarea class="field cz-desc" id="cz-d" rows="5" maxlength="2000" spellcheck="true" placeholder="${esc(t('Your own words about the game: what it is, how you play it, anything. Shown on its page.'))}">${esc(abouts[id] || '')}</textarea></div>`;
        const f = $('#cz-n', body);
        f.addEventListener('input', () => { const v = f.value.trim(); if (v && v !== own) names[id] = v; else delete names[id]; });
        const d = $('#cz-d', body);
        d.addEventListener('input', () => { const v = d.value.trim(); if (v) abouts[id] = v; else delete abouts[id]; });
        f.focus();
        return;
      }
      const spec = kindOf(k);
      body.innerHTML = `<div class="cz-art cz-${k}">
        <div class="cz-left">
          <div class="cz-stage" id="cz-stage"><canvas id="cz-cv"></canvas><div class="cz-empty" id="cz-empty"></div></div>
          <div class="cz-ctl">
            <label class="cz-zoom"><span>${esc(t('Zoom'))}</span><input type="range" id="cz-z" min="0" max="1000" step="1" aria-label="${esc(t('Zoom'))}"></label>
            <div class="cz-fit"><button type="button" class="btn sm" data-x="fit">${esc(t('Fit'))}</button><button type="button" class="btn sm" data-x="fill">${esc(t('Fill'))}</button><button type="button" class="btn sm" data-x="center">${esc(t('Center'))}</button></div>
          </div>
          <div class="cz-src grid3"><button type="button" class="btn" data-x="sgdb">SteamGridDB</button><button type="button" class="btn" data-x="file">${esc(t('Your Picture…'))}</button><button type="button" class="btn" data-x="reset">${esc(t('Use the Usual'))}</button></div>
        </div>
        <div class="cz-right"><h4>${esc(t('How it looks'))}</h4><div class="cz-prev" id="cz-prev"></div><p class="muted cz-tip">${esc(k === 'icon' ? t('Used for shortcuts, Steam and the game\'s window while it runs.') : k === 'logo' ? t('A see-through picture works best. It shows over the background.') : t('Drag the picture to move it. The frame is what shows.'))}</p></div>
      </div>`;
      stage(spec);
    }

    // The frame: the picture dragged and zoomed in it, what's outside the frame dimmed.
    function stage(spec) {
      const st = $('#cz-stage', box), cv = $('#cz-cv', box), z = $('#cz-z', box);
      if (!st) return;
      const e = ed[spec.k] && !ed[spec.k].reset ? ed[spec.k] : null;
      const sw = st.clientWidth, sh = st.clientHeight;
      const dpr = devicePixelRatio || 1;
      cv.width = Math.round(sw * dpr); cv.height = Math.round(sh * dpr);
      cv.style.width = sw + 'px'; cv.style.height = sh + 'px';
      // The frame, as big as fits with room around it.
      const pad = 26, ar = spec.w / spec.h;
      let fw = sw - pad * 2, fh = fw / ar;
      if (fh > sh - pad * 2) { fh = sh - pad * 2; fw = fh * ar; }
      const fx = (sw - fw) / 2, fy = (sh - fh) / 2;
      const ctx = cv.getContext('2d');
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, sw, sh);
      $('#cz-empty', box).hidden = !!e;
      $('#cz-empty', box).textContent = e ? '' : t('Pick a picture from SteamGridDB or your own to frame it here.');
      z.disabled = !e;
      for (const b of $$('[data-x="fit"], [data-x="fill"], [data-x="center"]', box)) b.disabled = !e;
      if (e) {
        // Checks behind see-through pictures.
        if (spec.type === 'image/png') {
          ctx.save(); ctx.beginPath(); ctx.rect(fx, fy, fw, fh); ctx.clip();
          for (let yy = 0; yy < fh; yy += 12) for (let xx = 0; xx < fw; xx += 12) { ctx.fillStyle = ((xx + yy) / 12) % 2 ? '#2a3140' : '#1e2430'; ctx.fillRect(fx + xx, fy + yy, 12, 12); }
          ctx.restore();
        }
        draw(ctx, e.img, spec, e, fx, fy, fw, fh);
        // Outside the frame, dimmed.
        ctx.fillStyle = 'rgba(8,10,16,.72)';
        ctx.fillRect(0, 0, sw, fy); ctx.fillRect(0, fy + fh, sw, sh - fy - fh); ctx.fillRect(0, fy, fx, fh); ctx.fillRect(fx + fw, fy, sw - fx - fw, fh);
        z.value = String(Math.round(((Math.log(e.zoom) - Math.log(ZMIN)) / (Math.log(ZMAX) - Math.log(ZMIN))) * 1000));
      }
      ctx.strokeStyle = 'rgba(52,242,232,.9)'; ctx.lineWidth = 2;
      ctx.strokeRect(fx, fy, fw, fh);
      st._frame = { fx, fy, fw, fh };
      previews(spec);
    }

    // Where it shows, with the game's other pictures as they'll be.
    function previews(spec) {
      const p = $('#cz-prev', box);
      if (!p) return;
      const pic = (k, cls) => { const c = current(k); return c.ed ? `<canvas class="${cls}" data-k="${k}"></canvas>` : c.url ? `<img class="${cls}" src="${esc(media(c.url))}" alt="">` : `<span class="${cls} ph"></span>`; };
      const nm = esc(names[id] || own);
      p.innerHTML = spec.k === 'cover' ? `<div class="pv-tile">${pic('cover', 'pv-cover')}<b>${nm}</b></div><div class="pv-row">${pic('cover', 'pv-cover')}<span>${nm}</span></div>`
        : spec.k === 'icon' ? `<div class="pv-icons">${pic('icon', 'pv-i64')}${pic('icon', 'pv-i32')}${pic('icon', 'pv-i16')}</div><div class="pv-bar">${pic('icon', 'pv-i24')}<span>${nm}</span></div>`
          : `<div class="pv-head">${pic('hero', 'pv-hero')}<div class="pv-shade"></div>${pic('logo', 'pv-logo')}</div><div class="pv-wide">${pic('hero', 'pv-hero')}<div class="pv-shade"></div>${pic('logo', 'pv-logo')}</div>`;
      for (const c of $$('canvas', p)) {
        const k = c.dataset.k, e = ed[k], sp = kindOf(k);
        const w = c.clientWidth || 100, h = c.clientHeight || 100;
        c.width = Math.round(w * (devicePixelRatio || 1)); c.height = Math.round(h * (devicePixelRatio || 1));
        const ctx = c.getContext('2d');
        // The shown box's shape may differ from the frame's: the framed picture as it would come out.
        if (k === 'logo') {
          const ar = sp.w / sp.h, bw = Math.min(c.width, c.height * ar), bh = bw / ar;
          draw(ctx, e.img, sp, e, (c.width - bw) / 2, (c.height - bh) / 2, bw, bh);
        } else draw(ctx, e.img, sp, e, 0, 0, c.width, c.height);
      }
    }

    // Dragging and zooming in the frame.
    let drag = null;
    box.addEventListener('pointerdown', (ev) => {
      const cv = ev.target.closest('#cz-cv');
      const e = ed[tab];
      if (!cv || !e || e.reset) return;
      drag = { x: ev.clientX, y: ev.clientY, ox: e.x, oy: e.y };
      cv.setPointerCapture(ev.pointerId);
    });
    box.addEventListener('pointermove', (ev) => {
      if (!drag) return;
      const e = ed[tab], f = $('#cz-stage', box)._frame;
      e.x = drag.ox + (ev.clientX - drag.x) / f.fw;
      e.y = drag.oy + (ev.clientY - drag.y) / f.fh;
      e.changed = true;
      requestAnimationFrame(() => stage(kindOf(tab)));
    });
    box.addEventListener('pointerup', () => { drag = null; });
    box.addEventListener('wheel', (ev) => {
      if (!ev.target.closest('#cz-stage')) return;
      const e = ed[tab];
      if (!e || e.reset) return;
      ev.preventDefault();
      e.zoom = Math.min(ZMAX, Math.max(ZMIN, e.zoom * Math.exp(-ev.deltaY * 0.0015)));
      e.changed = true;
      stage(kindOf(tab));
    }, { passive: false });
    box.addEventListener('input', (ev) => {
      if (ev.target.id !== 'cz-z') return;
      const e = ed[tab];
      if (!e) return;
      e.zoom = Math.exp(Math.log(ZMIN) + (Number(ev.target.value) / 1000) * (Math.log(ZMAX) - Math.log(ZMIN)));
      e.changed = true;
      stage(kindOf(tab));
    });
    // Arrow keys move it a little, + and - zoom.
    box.addEventListener('keydown', (ev) => {
      const e = ed[tab];
      if (!e || e.reset || !ev.target.closest('#cz-stage, #cz-z') || ev.target.id === 'cz-z' && (ev.key === 'ArrowLeft' || ev.key === 'ArrowRight')) return;
      const step = ev.shiftKey ? 0.05 : 0.01;
      const mv = { ArrowLeft: [-step, 0], ArrowRight: [step, 0], ArrowUp: [0, -step], ArrowDown: [0, step] }[ev.key];
      if (mv) { e.x += mv[0]; e.y += mv[1]; }
      else if (ev.key === '+' || ev.key === '=') e.zoom = Math.min(ZMAX, e.zoom * 1.06);
      else if (ev.key === '-') e.zoom = Math.max(ZMIN, e.zoom / 1.06);
      else return;
      ev.preventDefault();
      e.changed = true;
      stage(kindOf(tab));
    });
    const relayout = () => { if (tab !== 'name' && !browse) stage(kindOf(tab)); };
    addEventListener('resize', relayout);
    new MutationObserver(() => { if (!m.isConnected) removeEventListener('resize', relayout); }).observe(document.body, { childList: true });

    async function take(spec, from) {
      const body = $('#cz-body', box);
      body.classList.add('busy');
      try {
        const r = await invoke('art_source', Object.assign({ id, kind: spec.k }, from));
        ed[spec.k] = { src: r.src, img: await loadImg(r.data), x: 0, y: 0, zoom: 1, changed: true };
        show(spec.k);
      } catch (e) { fail(e); } finally { body.classList.remove('busy'); }
    }

    // SteamGridDB: the game found by its name (another can be searched for), and its pictures of
    // this kind to pick from.
    async function sgdb(spec) {
      const body = $('#cz-body', box);
      if (!key()) {
        body.innerHTML = `<div class="cz-key"><h4>${esc(t('SteamGridDB key'))}</h4><p class="muted">${esc(t('Pictures from SteamGridDB need your own free key, from steamgriddb.com (Preferences → API). It\'s kept on this PC.'))}</p>
          <div class="cz-keyrow"><input class="field" id="cz-key" type="password" autocomplete="off" spellcheck="false" placeholder="${esc(t('Paste your key'))}"><button type="button" class="btn main" data-x="key-save">${esc(t('Save'))}</button></div>
          <div class="acts grid2"><button type="button" class="btn" data-url="https://www.steamgriddb.com/profile/preferences/api">${esc(t('Get a Key'))}</button><button type="button" class="btn" data-x="back">${esc(t('Back'))}</button></div></div>`;
        return;
      }
      browse = browse || { term: a.game || own, games: null, sel: 0, art: {} };
      body.innerHTML = `<div class="cz-sg"><div class="cz-sgbar"><input class="field" id="cz-q" type="search" value="${esc(browse.term)}" spellcheck="false" aria-label="${esc(t('Search'))}"><button type="button" class="btn" data-x="sg-go">${esc(t('Search'))}</button><button type="button" class="btn" data-x="back">${esc(t('Back'))}</button></div>
        <div class="cz-games" id="cz-games"></div><div class="ap-grid ${spec.k === 'hero' ? 'hero' : spec.k === 'cover' ? 'cover' : spec.k === 'logo' ? 'logo' : 'icon'}" id="cz-grid"><p class="muted">${esc(t('Loading…'))}</p></div></div>`;
      const q = $('#cz-q', body);
      q.addEventListener('keydown', (ev) => { if (ev.key === 'Enter') { ev.preventDefault(); $('[data-x="sg-go"]', body).click(); } });
      // It looks as you type: a bit of the name is enough, then pick the game from the list.
      let tmr;
      q.addEventListener('input', () => {
        clearTimeout(tmr);
        tmr = setTimeout(async () => {
          const v = q.value.trim();
          if (v.length < 2 || !browse || v === browse.term) return;
          browse.term = v;
          let found = [];
          try { found = await invoke('art_search', { term: v, key: key() }); } catch (e) { /* the list stays */ }
          if (!browse || browse.term !== v) return;
          browse.games = found; browse.sel = 0;
          paintGames(spec);
        }, 400);
      });
      if (!browse.games) {
        try { browse.games = await invoke('art_search', { term: browse.term, key: key() }); } catch (e) { browse.games = []; $('#cz-grid', body).innerHTML = `<p class="muted">${esc(t(String(e && e.message ? e.message : e)))}</p>`; return; }
        browse.sel = 0;
      }
      paintGames(spec);
    }
    async function paintGames(spec) {
      const gl = $('#cz-games', box), grid = $('#cz-grid', box);
      if (!gl || !grid) return;
      gl.innerHTML = browse.games.slice(0, 8).map((x, i) => `<button type="button" class="chip${i === browse.sel ? ' on' : ''}" data-x="sg-game" data-i="${i}">${esc(x.name)}${x.year ? ` <small>${esc(x.year)}</small>` : ''}</button>`).join('');
      const game = browse.games[browse.sel];
      if (!game) { grid.innerHTML = `<p class="muted">${esc(t('SteamGridDB has nothing by that name. Try another.'))}</p>`; return; }
      let art = browse.art[game.id];
      if (!art) {
        grid.innerHTML = `<p class="muted">${esc(t('Loading…'))}</p>`;
        try { art = browse.art[game.id] = await invoke('art_for', { sgdb: game.id, key: key() }); } catch (e) { if (browse.games[browse.sel] === game) grid.innerHTML = `<p class="muted">${esc(t(String(e && e.message ? e.message : e)))}</p>`; return; }
        // Another game (or another search) was picked while this one loaded: that one draws.
        if (!$('#cz-grid', box) || browse.games[browse.sel] !== game || tab !== spec.k) return;
      }
      const list = art[spec.sgdb] || [];
      grid.style.setProperty('--cols', spec.k === 'hero' ? 2 : spec.k === 'cover' ? 5 : 4);
      grid.innerHTML = list.length ? list.map((x) => `<button type="button" class="ap-it" data-x="sg-pick" data-u="${esc(x.url)}"><img src="${esc(media(x.thumb))}" alt="" loading="lazy"></button>`).join('')
        : `<p class="muted">${esc(t('SteamGridDB has none of these for this game.'))}</p>`;
    }

    // A game that isn't installed (or a game's key) keeps its pictures beside the Library.
    function keep(x, r) {
      if (!r) return;
      if (!r.art_only) { S.library[x] = r; return; }
      delete r.art_only;
      if (Object.keys(r).length) S.arts[x] = r; else delete S.arts[x];
    }

    async function save(close) {
      const btn = $('[data-x="save"]', box);
      btn.disabled = true;
      try {
        const ui = S.settings.ui || {};
        // Ticked for every version: the same goes to each of them, and to the game's key for
        // versions still to come.
        const others = group ? [gameKey(a), ...group.every.map((v) => v.id).filter((x) => x !== id)] : [];
        if (every.has('name')) {
          for (const x of others) {
            if (names[id]) names[x] = names[id]; else delete names[x];
            if (abouts[id]) abouts[x] = abouts[id]; else delete abouts[x];
          }
        }
        if (JSON.stringify(ui.names || {}) !== JSON.stringify(names)) await saveUi({ names });
        if (JSON.stringify(ui.abouts || {}) !== JSON.stringify(abouts)) await saveUi({ abouts });
        for (const spec of KINDS) {
          const e = ed[spec.k];
          const all = every.has(spec.k);
          const mine = !!e && (e.changed || !!e.reset);
          if (!mine && !all) continue;
          // Ticked with nothing changed: this version's picture goes to the others as it is, and
          // with none of its own, the others go back to their usual ones too.
          const data = e && !e.reset ? renderArt(spec, e) : null;
          const frame = data ? { x: e.x, y: e.y, zoom: e.zoom } : null;
          if (mine) keep(id, await invoke('art_save', { id, kind: spec.k, data, src: data ? e.src : null, frame }));
          // The others get the finished picture only: the picture it was framed from stays this
          // version's, so changing it later here doesn't take it from under them.
          if (all) for (const x of others) keep(x, await invoke('art_save', { id: x, kind: spec.k, data, src: null, frame: null }));
        }
        byId(id);
        close();
        toast(t('Saved. It shows everywhere now, and in Steam the next time you add it.'));
        render(true);
        if (window.tvRefresh) window.tvRefresh();
      } catch (e) { fail(e); btn.disabled = false; }
    }

    async function onClick(x, close) {
      const spec = kindOf(tab);
      switch (x.dataset.x) {
        case 'tab': return show(x.dataset.k);
        case 'save': return save(close);
        case 'back': browse = null; return show(tab);
        case 'file': {
          const p = await pickFile({ title: t('A picture'), directory: false, multiple: false, filters: [{ name: t('Pictures'), extensions: ['png', 'jpg', 'jpeg', 'webp', 'gif', 'bmp'] }] }).catch(fail);
          if (p) take(spec, { path: typeof p === 'string' ? p : p.path });
          return;
        }
        case 'sgdb': return sgdb(spec);
        case 'sg-go': { const q = $('#cz-q', box).value.trim(); if (!q) return; browse = { term: q, games: null, sel: 0, art: browse ? browse.art : {} }; return sgdb(spec); }
        case 'sg-game': browse.sel = Number(x.dataset.i); return paintGames(spec);
        case 'sg-pick': return take(spec, { url: x.dataset.u });
        case 'key-save': { const v = ($('#cz-key', box) || {}).value || ''; if (!v.trim()) return; await saveUi({ sgdb_key: v.trim() }); return sgdb(spec); }
        case 'reset': {
          const had = (S.library[id] || (S.arts || {})[id] || {})[spec.k + '_file'];
          if (had) ed[spec.k] = Object.assign(ed[spec.k] || {}, { reset: true, changed: false });
          else delete ed[spec.k];
          return show(spec.k);
        }
        case 'fit': case 'fill': case 'center': {
          const e = ed[tab];
          if (!e || e.reset) return;
          const f = $('#cz-stage', box)._frame, p = place(e.img, spec, { x: 0, y: 0, zoom: 1 }, f.fw, f.fh);
          const base = spec.fill ? p.fill : p.fit;
          if (x.dataset.x === 'fit') e.zoom = p.fit / base;
          if (x.dataset.x === 'fill') e.zoom = p.fill / base;
          e.x = 0; e.y = 0;
          e.changed = true;
          return stage(spec);
        }
      }
    }
  };
})();
