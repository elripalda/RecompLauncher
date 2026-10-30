// English text in the launcher's headings, buttons, tabs and labels, in Title Case ("No Saves Yet",
// "1 Copy"): done on the page as it's drawn, for English only, so the other languages keep their own
// capitals. Only the launcher's own words are changed, never a game's or a developer's name.
(function () {
  const WORDS = new Set("a able about access account across actions activate add added adds after again against all allow almost already always an and anonymous another answer antivirus any anymore anything anywhere are aren aren't around art article as asked asks assisted at automatically available back backed backs backups banner be before being below beta between big bit black blocked blue bottom breaks bring broadcast broke browser bug build builds built busy but button buttons by can can't capture card certainly change changed changes chat check checked checks checksum choices choose clean cleared click clip clock close closed closes cloud code color com come comes coming community compare complete compressed computer connect connected connection console contrast controller controllers copied copies copy couch could couldn couldn't counts cover covers crashes crisp crosire curved dark date decide decides default delete deleted deletes desk desktop detail details developer developer's developers device diagnostics did didn didn't disc discover disk do does doesn doesn't doing done download downloaded downloading downloads draw drawn drive driver drives dump each easier else email empty enough errors every everything exactly exist expected extras faster feedback feels few file files filter find finds fine finish first fits five fix fixed flag flagged flat folder folders follow following for forward found free fresh from full game game's games games' genre get gets given glow go goes going gone grain graphics great guide had halos hand handheld happened happens harmful has hasn't have haven haven't hear help helped helping helps here hidden history holding holds home hour hours how icon icons idea if image images in incomplete info ini insert inside install installed installs instead instructions internet into is isn isn't it it's items its itself join json just keep keeps kept keyboard know known knows language larger last later latest launch launcher launcher's leaves left let lets library library's license like liked lines linked list listed little ll load loaded loader local logs long look looking looks lost lot made make many match matched matches may me means menu message min minute missing mistake mod mod's mode mods moment monitor month more mouse move moved moves music my name names need needs never new newer newest next no non none normal not notes nothing notification now of off offline old older on once one ones only open opened opening opens optional options or original other others out over own page part parts passed paste phone pick picked picker picks picture pinned place places play played player player's playing plays plugged popular press problem problems processor profile program programs progress project properly put puts quick quit quotes ran random rar re reach reached read reads ready rebuilt recent recomp recomp's recomps recompstore region release releases remove reply report reports reset rest restarts revision right run running runs runtime safe said same save saved saves say saying says scan scanlines scans screen screens screenshot screenshots search sec see select send sent session set sets setting settings setup share sharing sharp ship shortcut shortcuts should show shown shows side sidebar sign signed signing since size so soft software some something somewhere soon sound space spaces speed spots spread stars start started starter starts stats status stay stays step steps still stopped straight strong sure switch tab take taken takes tell ten test testers testing texture than thanks that that's the their them then there there's these they they're thing things this through time to today too took tools top total try tube tune turn turned two type uncompressed under understand unexpectedly uninstalled unless unpack unpacked unpacks until unusual up update updated updates us usage use used usual usually ve verified version versions view vignette virus voice vote wait waiting want was wasn't way we website week weeks went were what what's when where whether which while who whole whose why will window with without won won't words work working works worth would write writing wrong yesterday yet you you'd you're your yours yourself zip".split(' '));
  const SMALL = new Set(['a', 'an', 'the', 'and', 'but', 'or', 'nor', 'for', 'so', 'at', 'by', 'in', 'of', 'on', 'to', 'as', 'via', 'per', 'from', 'with', 'into', 'than', 'vs']);
  // Where it applies, and where it never does (names, paths, logs, sentences).
  const SEL = 'h1, h2, h3, h4, .btn, .linkbtn, .toggle, .seg button, .tabs-in button, .chip, .tag, .badge, .pill-sel, .set-tab span, .side .lbl, .menu button, .set-row .k > b, .card-h h3, .shelf-h h2, .sec-h, .stats span, .slot-n, .dl-when, .upd-when, .count-line, .tv-head h2, .tv-btn span, .tv-item .k, .tv-chip, .tv-pill, .tv-lh, .tv-sec h3, .tv-tab, .tv-cycle b, option';
  const SKIP = '.t, .ph, .pin, pre, code, textarea, input, small, p, .gp-title, .gp h1, .fb-path, .logbox, [data-raw], .tv-title, .tv-game-title';
  const fix = (s) => {
    const ws = [...s.matchAll(/[A-Za-z][A-Za-z'’]*/g)];
    if (!ws.length) return s;
    let out = '', at = 0;
    ws.forEach((m, i) => {
      const w = m[0], lo = w.toLowerCase(), end = m.index + w.length;
      let r = w;
      const dotted = s[end] === '.' && /[A-Za-z]/.test(s[end + 1] || '') || s[m.index - 1] === '.';
      if (w === lo && WORDS.has(lo) && !dotted && !(SMALL.has(lo) && i > 0 && i < ws.length - 1)) r = w[0].toUpperCase() + w.slice(1);
      out += s.slice(at, m.index) + r;
      at = end;
    });
    return out + s.slice(at);
  };
  const on = () => /^en\b/i.test(document.documentElement.lang || 'en');
  const host = (el) => {
    if (!el || el.closest(SKIP)) return null;
    if (el.matches(SEL)) return el;
    const up = el.parentElement;
    return up && /^(B|STRONG|EM|SPAN|LABEL)$/.test(el.tagName) && up.matches(SEL) ? up : null;
  };
  const doText = (n) => {
    if (!n.nodeValue || !/[a-z]/.test(n.nodeValue) || !host(n.parentElement)) return;
    const v = fix(n.nodeValue);
    if (v !== n.nodeValue) n.nodeValue = v;
  };
  const walk = (root) => {
    if (root.nodeType === 3) return doText(root);
    if (root.nodeType !== 1) return;
    const it = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
    for (let n = it.nextNode(); n; n = it.nextNode()) doText(n);
  };
  const mo = new MutationObserver((recs) => {
    if (!on()) return;
    for (const r of recs) {
      if (r.type === 'characterData') doText(r.target);
      else for (const n of r.addedNodes) walk(n);
    }
  });
  const start = () => { if (on()) walk(document.body); mo.observe(document.body, { childList: true, subtree: true, characterData: true }); };
  if (document.body) start(); else document.addEventListener('DOMContentLoaded', start);
  window.titleCase = fix;
})();
