// The launcher in the player's language: the same 24 as recomplauncher.com, the same way.
// Every English text in the window goes through t(), tf() or tn(); i18n/<code>.json holds a
// language's {"English": "translation"}. tools/launcher-i18n.py lists what's left to translate.
(() => {
  'use strict';
  const LANGS = [['en', 'English (US)'], ['en-GB', 'English (UK)'], ['es', 'Español (España)'], ['es-419', 'Español (Latinoamérica)'],
    ['fr', 'Français (France)'], ['fr-CA', 'Français (Canada)'], ['pt', 'Português (Portugal)'], ['pt-BR', 'Português (Brasil)'],
    ['it', 'Italiano'], ['de', 'Deutsch'], ['nl', 'Nederlands'], ['pl', 'Polski'], ['tr', 'Türkçe'], ['ru', 'Русский'], ['uk', 'Українська'],
    ['vi', 'Tiếng Việt'], ['id', 'Bahasa Indonesia'], ['th', 'ไทย'], ['ar', 'العربية'], ['ja', '日本語'], ['zh-Hans', '简体中文'], ['zh-Hant', '繁體中文'], ['ko', '한국어']];
  const CODES = LANGS.map((l) => l[0]);
  // A regional language shows its base language's text where it has none of its own.
  const BASE = { 'en-GB': 'en', 'es-419': 'es', 'fr-CA': 'fr', 'pt-BR': 'pt' };
  let lang = 'en';
  let dict = {};
  const has = (s) => Object.prototype.hasOwnProperty.call(dict, s) && dict[s];
  const t = (s) => (s == null ? '' : has(s) ? dict[s] : s);
  // English in a table that t() is given later.
  const N_ = (s) => s;
  const locale = () => (lang === 'ar' ? 'ar-u-nu-latn' : lang);
  const fmtNum = (n, digits) => {
    try { return new Intl.NumberFormat(locale(), digits == null ? {} : { minimumFractionDigits: digits, maximumFractionDigits: digits }).format(n); }
    catch (e) { return digits == null ? String(n) : Number(n).toFixed(digits); }
  };
  // A count's form: Polish, Russian and Ukrainian "one|few|many" (21 takes "one"), Arabic "two|few|many|other".
  function pickForm(text, n) {
    if (text.indexOf('|') < 0 || n == null) return text;
    const f = text.split('|'), a = Math.abs(Number(n)) || 0, d = a % 10, h = a % 100, base = BASE[lang] || lang;
    let i = 0;
    if (base === 'ru' || base === 'uk') i = d === 1 && h !== 11 ? 0 : d >= 2 && d <= 4 && (h < 12 || h > 14) ? 1 : 2;
    else if (base === 'pl') i = a === 1 ? 0 : d >= 2 && d <= 4 && (h < 12 || h > 14) ? 1 : 2;
    else if (base === 'ar' && f.length === 4) i = a === 2 ? 0 : h >= 3 && h <= 10 ? 1 : h >= 11 && h <= 99 ? 2 : 3;
    return f[Math.min(i, f.length - 1)];
  }
  // tf('Updated {date}', {date: x}): translated, then each {name} filled (numbers in the language's
  // way). Values go in as they are: in HTML, escape them first.
  const tf = (text, v) => pickForm(t(text), v && v.n).replace(/\{(\w+)\}/g, (m, k) => (v && k in v ? (typeof v[k] === 'number' ? fmtNum(v[k]) : String(v[k])) : m));
  // tn(3, '1 game', '{n} games')
  const tn = (n, one, many) => tf(n === 1 ? one : many, { n });

  function matchLang(tag) {
    const parts = String(tag || '').toLowerCase().split(/[-_]/);
    const base = parts[0];
    const region = parts.slice(1).find((x) => x.length === 2 || /^\d{3}$/.test(x)) || '';
    const script = parts.slice(1).find((x) => x.length === 4) || '';
    if (base === 'en') return ['gb', 'ie', 'au', 'nz'].includes(region) ? 'en-GB' : 'en';
    if (base === 'es') return !region || region === 'es' ? 'es' : 'es-419';
    if (base === 'fr') return region === 'ca' ? 'fr-CA' : 'fr';
    if (base === 'pt') return region === 'br' ? 'pt-BR' : 'pt';
    if (base === 'zh') return script === 'hant' || ['tw', 'hk', 'mo'].includes(region) ? 'zh-Hant' : 'zh-Hans';
    return CODES.includes(base) ? base : '';
  }
  // The computer's language, when it's one of ours; else English.
  function systemLang() {
    const list = navigator.languages && navigator.languages.length ? navigator.languages : [navigator.language || 'en'];
    for (const tag of list) { const m = matchLang(tag); if (m) return m; }
    return 'en';
  }
  // A language's text (its base language's first); English when a file can't be read.
  async function loadLang(code) {
    if (!CODES.includes(code)) code = 'en';
    const files = code === 'en' ? [] : BASE[code] && BASE[code] !== 'en' ? [BASE[code], code] : [code];
    const core = (c) => (window.__TAURI__ ? window.__TAURI__.core.invoke('ui_data', { name: c }) : Promise.reject());
    const get = (c) => fetch(`i18n/${c}.json`).then((r) => (r.ok ? r.json() : Promise.reject(r.status))).catch(() => core(c)).catch(() => ({}));
    const ds = await Promise.all(files.map(get));
    dict = Object.assign({}, ...ds.map((d) => (d && typeof d === 'object' ? d : {})));
    lang = code;
    document.documentElement.lang = code;
    document.documentElement.dir = code === 'ar' ? 'rtl' : 'ltr';
    return code;
  }
  // The static parts of a page: elements marked data-t get their own text, title, aria-label and
  // placeholder translated. The English is kept, so a second language can be put on later.
  const orig = new WeakMap();
  function tDom(root) {
    for (const el of (root || document).querySelectorAll('[data-t]')) {
      let o = orig.get(el);
      if (!o) {
        o = { nodes: [...el.childNodes].filter((n) => n.nodeType === 3 && n.nodeValue.trim()).map((n) => [n, n.nodeValue.trim()]), attrs: {} };
        for (const a of ['title', 'aria-label', 'placeholder']) if (el.hasAttribute(a)) o.attrs[a] = el.getAttribute(a);
        orig.set(el, o);
      }
      for (const [n, en] of o.nodes) n.nodeValue = n.nodeValue.replace(n.nodeValue.trim(), t(en));
      for (const a in o.attrs) el.setAttribute(a, t(o.attrs[a]));
    }
  }
  Object.assign(window, { tDom, t, tf, tn, N_, fmtNum, LANGS, matchLang, systemLang, loadLang, curLang: () => lang, curLocale: locale, isRtl: () => lang === 'ar' });
})();
