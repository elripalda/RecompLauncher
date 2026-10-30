'use strict';
// The page's scrollbar: thin, over the page, below the window buttons, and only there while you
// scroll (or point at it or drag it). It fades away when you stop.
(() => {
  const main = document.getElementById('main');
  const wrap = main && main.parentElement;
  if (!main || !wrap) return;
  const bar = document.createElement('div');
  bar.className = 'vbar';
  bar.setAttribute('aria-hidden', 'true');
  bar.innerHTML = '<i></i>';
  wrap.appendChild(bar);
  const thumb = bar.firstChild;
  let hideT = 0, drag = null, raf = 0;

  function place() {
    raf = 0;
    const h = main.clientHeight, sh = main.scrollHeight, track = bar.clientHeight;
    if (sh <= h + 1 || !track) { bar.classList.add('none'); return; }
    bar.classList.remove('none');
    const th = Math.max(36, Math.round(track * (h / sh)));
    const y = Math.round((track - th) * (main.scrollTop / (sh - h)));
    thumb.style.height = th + 'px';
    thumb.style.transform = `translateY(${y}px)`;
  }
  const soon = () => { if (!raf) raf = requestAnimationFrame(place); };
  function show() {
    soon();
    bar.classList.add('on');
    clearTimeout(hideT);
    hideT = setTimeout(() => { if (!drag && !bar.matches(':hover')) bar.classList.remove('on'); }, 900);
  }
  main.addEventListener('scroll', show, { passive: true });
  addEventListener('resize', soon);
  // A new page: the size is measured again, without showing the bar.
  new ResizeObserver(soon).observe(main);
  new MutationObserver(soon).observe(main, { childList: true });
  bar.addEventListener('mouseenter', show);
  bar.addEventListener('mouseleave', show);
  thumb.addEventListener('pointerdown', (e) => {
    e.preventDefault();
    thumb.setPointerCapture(e.pointerId);
    drag = { y: e.clientY, top: main.scrollTop };
    bar.classList.add('drag', 'on');
  });
  thumb.addEventListener('pointermove', (e) => {
    if (!drag) return;
    const h = main.clientHeight, sh = main.scrollHeight, track = bar.clientHeight, th = thumb.offsetHeight;
    main.scrollTop = drag.top + ((e.clientY - drag.y) * (sh - h)) / Math.max(1, track - th);
  });
  const end = () => { if (!drag) return; drag = null; bar.classList.remove('drag'); show(); };
  thumb.addEventListener('pointerup', end);
  thumb.addEventListener('pointercancel', end);
  // A click on the track pages up or down, like a normal scrollbar.
  bar.addEventListener('pointerdown', (e) => {
    if (e.target !== bar) return;
    const r = thumb.getBoundingClientRect();
    main.scrollBy({ top: (e.clientY < r.top ? -1 : 1) * main.clientHeight * 0.9, behavior: 'smooth' });
  });
  soon();
})();
