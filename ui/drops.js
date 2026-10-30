'use strict';
// Every <select class="sel"> gets a themed button and list in place of the system one.
// The real select stays in the page (hidden) and still sends 'change', so nothing else changes.
(() => {
  const CHEV = '<svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><path d="m2.5 4.5 3.5 3.5 3.5-3.5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/></svg>';
  const TICK = '<svg width="15" height="15" viewBox="0 0 24 24" aria-hidden="true"><path d="m5 12.5 4.5 4.5L19 7.5" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>';
  const escH = (s) => String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
  let open = null; // { sel, btn, menu }

  function nameOf(sel) {
    if (sel.getAttribute('aria-label')) return sel.getAttribute('aria-label');
    const pill = sel.closest('.pill-sel');
    if (pill) return (pill.firstChild && pill.firstChild.nodeType === 3 ? pill.firstChild.textContent : pill.textContent).trim();
    const row = sel.closest('.set-row');
    const h = row && row.querySelector('b, h4, strong');
    return h ? h.textContent.trim() : '';
  }
  function label(sel) {
    const o = sel.options[sel.selectedIndex];
    return o ? o.textContent : '';
  }
  function sync(sel) {
    const btn = sel.__drop;
    if (!btn) return;
    const txt = label(sel);
    const span = btn.firstElementChild;
    if (span.textContent !== txt) span.textContent = txt;
    btn.disabled = sel.disabled;
    const n = nameOf(sel);
    btn.setAttribute('aria-label', n ? n + ': ' + txt : txt);
  }
  function dress(sel) {
    if (sel.__drop || sel.closest('.tv, #tv')) return;
    const btn = document.createElement('button');
    btn.type = 'button';
    btn.className = 'drop' + (sel.closest('.pill-sel') ? ' in-pill' : '');
    btn.setAttribute('aria-haspopup', 'listbox');
    btn.setAttribute('aria-expanded', 'false');
    btn.innerHTML = '<span></span>' + CHEV;
    sel.__drop = btn;
    btn.__sel = sel;
    sel.classList.add('sel-hidden');
    sel.tabIndex = -1;
    sel.setAttribute('aria-hidden', 'true');
    sel.after(btn);
    sync(sel);
  }
  function dressAll(root) {
    for (const s of (root || document).querySelectorAll('select.sel')) {
      if (!s.__drop) dress(s);
      else sync(s);
    }
  }
  window.syncDrops = () => dressAll(document);

  function close(focusBack) {
    if (!open) return;
    const { btn, menu } = open;
    open = null;
    btn.setAttribute('aria-expanded', 'false');
    btn.classList.remove('open');
    menu.classList.add('out');
    setTimeout(() => menu.remove(), 120);
    if (focusBack && btn.isConnected) btn.focus({ preventScroll: true });
  }
  function place(btn, menu) {
    const pill = btn.closest('.pill-sel');
    const a = (pill || btn).getBoundingClientRect();
    const w = Math.max(a.width, 200);
    menu.style.minWidth = w + 'px';
    const mw = menu.offsetWidth, mh = menu.offsetHeight;
    let x = Math.min(a.left, innerWidth - mw - 10);
    x = Math.max(10, x);
    let y = a.bottom + 6;
    if (y + mh > innerHeight - 10 && a.top - mh - 6 > 10) { y = a.top - mh - 6; menu.classList.add('up'); }
    menu.style.left = x + 'px';
    menu.style.top = y + 'px';
    menu.style.maxHeight = Math.max(160, Math.min(380, (menu.classList.contains('up') ? a.top : innerHeight - a.bottom) - 24)) + 'px';
  }
  function show(btn, keyboard) {
    const sel = btn.__sel;
    if (!sel || sel.disabled) return;
    if (open && !open.menu.isConnected) { open.btn.setAttribute('aria-expanded', 'false'); open.btn.classList.remove('open'); open = null; }
    if (open && open.btn === btn) return close(true);
    close();
    if (typeof closeMenu === 'function') closeMenu();
    const menu = document.createElement('div');
    menu.className = 'menu dropmenu';
    menu.setAttribute('role', 'listbox');
    menu.setAttribute('aria-label', nameOf(sel));
    // A long list gets a search box at the top.
    const long = sel.options.length > 10;
    menu.innerHTML = (long ? `<input class="drop-q" type="search" placeholder="${escH(typeof t === 'function' ? t('Search') : 'Search')}" aria-label="${escH(typeof t === 'function' ? t('Search') : 'Search')}" spellcheck="false">` : '') + [...sel.options].map((o, i) =>
      `<button type="button" role="option" data-i="${i}" aria-selected="${o.selected}"${o.disabled ? ' disabled' : ''}><span>${escH(o.textContent)}</span>${o.selected ? `<i class="ck">${TICK}</i>` : ''}</button>`).join('');
    document.body.appendChild(menu);
    place(btn, menu);
    open = { sel, btn, menu };
    btn.setAttribute('aria-expanded', 'true');
    btn.classList.add('open');
    const cur = menu.querySelector('[aria-selected="true"]') || menu.querySelector('button:not([disabled])');
    if (cur) {
      cur.scrollIntoView({ block: 'nearest' });
      if (keyboard && !long) cur.focus({ preventScroll: true });
      else cur.classList.add('hot');
    }
    const q = menu.querySelector('.drop-q');
    if (q) {
      q.focus({ preventScroll: true });
      q.addEventListener('input', () => {
        const v = q.value.trim().toLowerCase();
        for (const b of menu.querySelectorAll('button[role="option"]')) b.hidden = !!v && !b.textContent.toLowerCase().includes(v);
        for (const h of menu.querySelectorAll('.hot')) h.classList.remove('hot');
        const first = menu.querySelector('button[role="option"]:not([hidden]):not([disabled])');
        if (first) { first.classList.add('hot'); first.scrollIntoView({ block: 'nearest' }); }
      });
      q.addEventListener('keydown', (e) => {
        const first = menu.querySelector('button[role="option"]:not([hidden]):not([disabled])');
        if (e.key === 'ArrowDown' && first) { e.preventDefault(); e.stopPropagation(); first.focus(); }
        else if (e.key === 'Enter') { e.preventDefault(); e.stopPropagation(); const h = menu.querySelector('button.hot:not([hidden])') || first; if (h) pick(Number(h.dataset.i)); }
        else if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); close(true); }
      });
    }
    menu.addEventListener('mousemove', (e) => {
      const b = e.target.closest('button');
      if (b && !b.classList.contains('hot')) { for (const h of menu.querySelectorAll('.hot')) h.classList.remove('hot'); b.classList.add('hot'); }
    });
  }
  function pick(i) {
    if (!open) return;
    const { sel } = open;
    close(true);
    if (sel.selectedIndex === i) return;
    sel.selectedIndex = i;
    sync(sel);
    sel.dispatchEvent(new Event('change', { bubbles: true }));
  }

  document.addEventListener('click', (e) => {
    const opt = e.target.closest('.dropmenu [role="option"]');
    if (opt) { e.stopPropagation(); if (!opt.disabled) pick(Number(opt.dataset.i)); return; }
    if (e.target.closest('.dropmenu')) { e.stopPropagation(); return; }
    let btn = e.target.closest('.drop');
    if (!btn) {
      const pill = e.target.closest('.pill-sel');
      if (pill) { e.preventDefault(); btn = pill.querySelector('.drop'); }
    }
    if (btn) { e.preventDefault(); e.stopPropagation(); show(btn, e.detail === 0); return; }
    if (open) close();
  }, true);

  document.addEventListener('keydown', (e) => {
    if (open && open.menu.contains(e.target)) {
      const items = [...open.menu.querySelectorAll('button:not([disabled]):not([hidden])')];
      const q = open.menu.querySelector('.drop-q');
      if (q && e.target === q) return;
      if (q && e.key === 'ArrowUp' && items.indexOf(e.target) === 0) { e.preventDefault(); e.stopPropagation(); q.focus(); return; }
      if (q && e.key.length === 1 && !e.ctrlKey && !e.metaKey) { q.focus(); return; }
      const at = items.indexOf(e.target);
      const go = (n) => { const b = items[Math.max(0, Math.min(items.length - 1, n))]; if (b) b.focus(); };
      if (e.key === 'ArrowDown') go(at + 1);
      else if (e.key === 'ArrowUp') go(at - 1);
      else if (e.key === 'Home') go(0);
      else if (e.key === 'End') go(items.length - 1);
      else if (e.key === 'Escape') close(true);
      else if (e.key === 'Tab') { close(true); return; }
      else if (e.key === 'Enter' || e.key === ' ') pick(Number(e.target.dataset.i));
      else if (e.key.length === 1) {
        const k = e.key.toLowerCase();
        const b = items.slice(at + 1).concat(items.slice(0, at + 1)).find((x) => x.textContent.trim().toLowerCase().startsWith(k));
        if (b) b.focus();
      } else return;
      e.preventDefault(); e.stopPropagation();
      return;
    }
    const btn = e.target.closest && e.target.closest('.drop');
    if (btn && (e.key === 'ArrowDown' || e.key === 'ArrowUp')) { e.preventDefault(); show(btn, true); }
  }, true);

  addEventListener('resize', () => close());
  document.addEventListener('scroll', (e) => { if (open && !open.menu.contains(e.target)) close(); }, true);
  // Selects added by any render get dressed straight away.
  new MutationObserver((list) => {
    for (const m of list) for (const n of m.addedNodes) {
      if (n.nodeType !== 1) continue;
      if (n.matches('select.sel')) dress(n);
      else if (n.querySelector) for (const s of n.querySelectorAll('select.sel')) dress(s);
    }
    if (open && !open.btn.isConnected) close();
  }).observe(document.documentElement, { childList: true, subtree: true });
  dressAll(document);
})();
