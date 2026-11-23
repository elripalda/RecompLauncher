const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const read = (f) => fs.readFileSync(path.join(__dirname, '..', f), 'utf8');
const app = read('ui/app.js');
const tv = read('ui/tv.js');
const fn = (src, name) => src.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^}`, 'm'))[0];
const same = (x) => x;

// The count's part of app.js (WANT, loadWant, wantCount, wantTag) with a stand-in website.
function want(answer) {
  const part = app.match(/^const WANT = \{\};[^]*?^const wantTag = [^\n]*/m)[0];
  const asked = [];
  const S = { view: 'discover' };
  const invoke = async (cmd, args) => { asked.push([cmd, args.id]); return answer(args.id); };
  const run = new Function('invoke', 'S', 'refreshGame', 'window', 'document', 'esc', 'tf', 'fmtNum', `${part}; return { WANT, wantCount, wantTag, loadWant };`);
  const tf = (s, v) => s.replace('{n}', v.n);
  const fmt = (n) => Number(n).toLocaleString('en-US');
  const api = run(invoke, S, () => {}, {}, { body: { classList: { contains: () => false } } }, same, tf, fmt);
  return Object.assign(api, { asked });
}
const settle = () => new Promise((r) => setTimeout(r, 0));

test('a count is asked for once, then shown', async () => {
  const w = want(() => 3412);
  assert.equal(w.wantCount('leonida'), 0);
  assert.equal(w.wantCount('leonida'), 0);
  await settle();
  assert.deepEqual(w.asked, [['want_count', 'leonida']]);
  assert.equal(w.wantCount('leonida'), 3412);
});

test('under 10 is not shown, like the website', async () => {
  const w = want(() => 9);
  w.wantCount('a');
  await settle();
  assert.equal(w.wantCount('a'), 0);
  const ten = want(() => 10);
  ten.wantCount('a');
  await settle();
  assert.equal(ten.wantCount('a'), 10);
});

test('a website that cannot say shows nothing', async () => {
  const w = want(() => { throw new Error('400'); });
  w.wantCount('a');
  await settle();
  assert.equal(w.wantCount('a'), 0);
});

test('an answer older than 10 minutes is asked for again, and shown meanwhile', async () => {
  const w = want(() => 50);
  w.wantCount('a');
  await settle();
  w.WANT.a.at -= 601e3;
  assert.equal(w.wantCount('a'), 50);
  await settle();
  assert.equal(w.asked.length, 2);
});

test('the number follows the label, with the full sentence on hover', () => {
  const w = want(() => 0);
  assert.equal(w.wantTag(0), '');
  const tag = w.wantTag(3412);
  assert.match(tag, /^<span class="want" title="3,412 people saved it or are waiting for it">3,412<\/span>$/);
});

test('Notify Me and Add to Library carry the count on the game page only', () => {
  const acts = fn(app, 'actions');
  assert.match(acts, /function actions\(a, size = 'big', \{ want = false \} = \{\}\)/);
  assert.match(acts, /want && !a\.custom \? wantTag\(wantCount\(a\.id\)\)/);
  assert.match(acts, /'Notify Me'\)\), BELL\(18\), wants\(\)\)/);
  assert.match(acts, /'We\\'ll Email You'\)\), BELL\(18\), wants\(\)\)/);
  assert.match(acts, /'Add to Library'\)\), PLUS, wants\(\)\)/);
  // Install, Play and the rest have no count.
  assert.equal((acts.match(/wants\(\)\)/g) || []).length, 3);
  // The game page asks for it; Home's banners (slideActs) don't.
  assert.equal((app.match(/actions\(a, 'big', \{ want: true \}\)/g) || []).length, 2);
  assert.doesNotMatch(fn(app, 'slideActs'), /want/);
});

test('Big Picture shows the same count on its game page', () => {
  assert.match(tv, /function mainButton\(a, key, want = false\)/);
  assert.match(tv, /mainButton\(a, 'g-main', true\)/);
  assert.match(tv, /mainButton\(a, 'hero-main'\)/);
  assert.equal((tv.match(/\)\) \+ wants\(\)/g) || []).length, 3);
});

test('every language has the hover sentence', () => {
  const key = '{n} people saved it or are waiting for it';
  for (const f of fs.readdirSync(path.join(__dirname, '../ui/i18n'))) {
    if (!f.endsWith('.json') || f === 'en-GB.json') continue;
    const lang = JSON.parse(read(`ui/i18n/${f}`));
    assert.ok(lang[key] && lang[key].includes('{n}'), f);
  }
});

test('the launcher answers want_count', () => {
  const lib = read('src-tauri/src/lib.rs');
  assert.match(lib, /async fn want_count\(/);
  assert.match(lib, /play_reports, want_count,/);
});
