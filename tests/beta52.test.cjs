const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const read = (f) => fs.readFileSync(path.join(__dirname, '../ui', f), 'utf8');
const app = read('app.js'), tv = read('tv.js'), named = read('setup-inputs.js'), css = read('app.css'), tvcss = read('tv.css'), html = read('index.html');
const MediaArt = require('../ui/media-art.js');
const fn = (src, name) => src.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];

test('every console has its own media, and anything else is a disc', () => {
  for (const [origin, kind] of [['n64', 'n64'], ['switch', 'switch'], ['psp', 'umd'], ['gc', 'gc'], ['ps4', 'bluray'], ['x360', 'dvd'], ['gbc', 'gbc'], ['pc', 'dvd']]) {
    assert.equal(MediaArt.kindOf(origin), kind, origin);
  }
  for (const k of MediaArt.KINDS) {
    const out = MediaArt.render(k.id, { cover: 'c.jpg', hero: 'h.jpg', logo: 'l.png' });
    assert.match(out, new RegExp(`class="md md-(cart|disc|umd) md-k-${k.id}"`), k.id);
    assert.equal((out.match(/<svg /g) || []).length, (out.match(/<\/svg>/g) || []).length, k.id);
    assert.match(MediaArt.ghost(k.id), /md-ghost/);
  }
  // A kind the site names that the launcher doesn't know falls back to the disc.
  assert.match(MediaArt.render('n64', { design: { kind: 'nope' } }), /md-k-n64/);
  assert.match(MediaArt.render('nope', {}), /md-k-dvd/);
});

test('discs and the UMD turn on their own layer; the white UMD shell stays put', () => {
  const disc = MediaArt.render('bluray', { logo: 'l.png' });
  assert.match(disc, /md-l md-spin/);
  const umd = MediaArt.render('umd', { logo: 'l.png' });
  const spin = umd.match(/<svg class="md-l md-spin"[^]*?<\/svg>/)[0];
  assert.doesNotMatch(spin, /M 244,195/, 'the UMD lettering is on the shell, not the disc');
  assert.match(umd.match(/<svg class="md-l md-front"[^]*?<\/svg>/)[0], /M 244,195[^"]*" fill="none" stroke="#111214"/);
  // Cartridges are one still layer: they slide and click instead.
  assert.doesNotMatch(MediaArt.render('n64', {}), /md-spin/);
});

test("defaults: covers on cartridges, the backdrop's colour on discs, no logo on the GameCube", () => {
  assert.equal(MediaArt.designFor('n64', {}).art, 'cover');
  assert.equal(MediaArt.designFor('n64', {}).logo.on, false);
  assert.equal(MediaArt.designFor('snes', {}).art, 'hero');
  assert.equal(MediaArt.designFor('snes', {}).logo.on, true);
  assert.equal(MediaArt.designFor('bluray', {}).style, 'color');
  assert.equal(MediaArt.designFor('bluray', {}).logo.on, true);
  assert.equal(MediaArt.designFor('gc', {}).logo.on, false);
  // A link the design names but doesn't give falls back to the game's own art.
  assert.equal(MediaArt.designFor('n64', { design: { art: 'url' } }).art, 'cover');
});

test("a disc's logo fits inside the band above the hub, whatever its shape", () => {
  for (const a of [.5, 1, 2, 4.2, 8]) {
    const lg = MediaArt.ringLogo(66, 177, a), D = 354;
    const w = lg.s * D, h = w / a, cx = 300, top = 210 - 177 + lg.y * D - h / 2, bot = top + h;
    // The top corners stay inside the print, and the bottom stays above the hub.
    assert.ok(Math.hypot(w / 2, 210 - top) <= 177 + .01, `a=${a} top`);
    assert.ok(bot <= 210 - 66 + .01, `a=${a} bottom`);
    assert.equal(cx, 300);
  }
});

test('a saved crop places the picture exactly; the print colour keeps the logo readable', () => {
  const out = MediaArt.render('switch', { cover: 'c.jpg', design: { crop: { x: .5, y: 1, z: 1, w: 400, h: 600 } } });
  // The 210 x 294 label: 400 x 600 scales by .525 to 210 x 315, and y = 1 puts its bottom on the label's.
  assert.match(out, /<image href="c\.jpg" x="195" y="33" width="210" height="315" preserveAspectRatio="none"\/>/);
  const zoomed = MediaArt.render('switch', { cover: 'c.jpg', design: { crop: { x: 0, y: 0, z: 2, w: 400, h: 600 } } });
  assert.match(zoomed, /<image href="c\.jpg" x="195" y="54" width="420" height="630"/);
  // A light logo gets a deep print; a dark logo a light one.
  const light = (c) => { const n = parseInt(c.slice(1), 16); return ((n >> 16) * .299 + ((n >> 8) & 255) * .587 + (n & 255) * .114) / 255; };
  assert.ok(light(MediaArt.printColour('#94d9d8', .7)) < .5);
  assert.ok(light(MediaArt.printColour('#2c3c5c', .1)) > .5);
});

test('the setup screens show the media in place of the old disc, desktop and Big Picture', () => {
  assert.match(html, /<link rel="stylesheet" href="media-art\.css">/);
  assert.ok(html.indexOf('media-art.js') < html.indexOf('app.js'));
  const su = fn(app, 'suDraw');
  assert.match(su, /const mdState = SU\.stage === 'pick' \? 'ghost' : SU\.stage === 'convert' \? 'is-busy'/);
  assert.match(su, /<div class="su-media">\$\{gameMediaHtml\(a, mdState\)\}<\/div>/);
  assert.match(su, /mediaState\(d, 'is-busy'\)/);
  assert.match(named, /\$\{gameMediaHtml\(game, selected \? '' : 'ghost'\)\}/);
  assert.match(tv, /const med = \(state\) => `<div class="tv-media">\$\{gameMediaHtml\(a, state\)\}<\/div>`/);
  assert.match(tv, /\$\{med\(cur <= 0 \? 'is-in' : 'is-busy'\)\}/);
  for (const src of [app, tv, named, css, tvcss]) assert.doesNotMatch(src, /sdisc|bigdisc/);
  // The game's design from the catalog goes straight to the drawing.
  assert.match(fn(named, 'gameMediaArt'), /design: a\.media_design && typeof a\.media_design === 'object' && !Array\.isArray\(a\.media_design\) \? a\.media_design : null/);
  // A game's "Around the web" links (its media array) are never read as a design.
  const art = new Function('media', 'thumbOf', `${fn(named, 'gameMediaArt')}; return gameMediaArt;`)((u) => u, (u) => u);
  assert.equal(art({ media: [{ url: 'https://example.dev' }] }).design, null);
  assert.equal(art({ media_design: [] }).design, null);
  assert.deepEqual(art({ media_design: { style: 'color' } }).design, { style: 'color' });
});

test('Console Labels on Covers: a setting, the site\'s PNG over the top, badges moved under it', () => {
  assert.match(app, /sw\('cover_labels', ui\.cover_labels, t\('Console Labels on Covers'\), 'ui-toggle'\)/);
  const S = { settings: { ui: {} }, cat: { labels: { x360: { src: 'https://x/l.png', s: 1, y: 0, h: .1 } } } };
  const esc = (s) => String(s), media = (u) => u;
  const label = new Function('S', 'esc', 'media', `${fn(app, 'coverLabel')}; ${fn(app, 'coverLabelPush')}; return [coverLabel, coverLabelPush];`)(S, esc, media);
  assert.equal(label[0]({ origin: 'x360' }), '', 'off unless the player turns it on');
  S.settings.ui.cover_labels = true;
  assert.match(label[0]({ origin: 'x360' }), /<img class="cvl" src="https:\/\/x\/l\.png"[^>]*--cvl-s:1;--cvl-y:0/);
  assert.equal(label[0]({ origin: 'n64' }), '');
  assert.equal(label[1]({ origin: 'x360' }), ' cvl-on" style="--cvl-h:0.1;--cvl-top:0.1');
  S.cat.labels.x360.push = true;
  assert.match(label[1]({ origin: 'x360' }), /cvl-on cvl-push/);
  assert.match(fn(app, 'poster'), /class="poster\$\{lb \? ' local' : ''\}\$\{coverLabelPush\(a\)\}"[^]*\$\{coverLabel\(a\)\}/);
  assert.match(tv, /class="tv-poster\$\{coverLabelPush\(a\)\}"/);
  assert.match(css, /\.poster img\.cvl,\.tv-poster img\.cvl\{/);
  assert.match(css, /\.poster\.cvl-on \.badge\.tl,\.poster\.cvl-on \.badge\.tr,\.poster\.cvl-on \.badge\.rank\{top:calc\(var\(--cvl-top\) \* 100% \+ 9px\)\}/);
});

test('the media moves only when the player allows motion', () => {
  const m = read('media-art.css');
  assert.match(m, /@media \(prefers-reduced-motion:reduce\)\{\.md,\.md \*\{animation:none!important\}\}/);
  assert.match(m, /\.md-disc\.is-busy \.md-spin,\.md-umd\.is-busy \.md-spin\{animation:mdSpin/);
  assert.match(m, /\.md-cart\.is-in\{animation:mdSlide/);
});
