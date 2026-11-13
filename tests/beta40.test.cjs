const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const read = (f) => fs.readFileSync(path.join(__dirname, '../ui', f), 'utf8');
const app = read('app.js'), css = read('app.css');

// A const arrow function out of app.js, by name, to run on its own.
function arrow(src, name) {
  const start = src.indexOf(`const ${name} = `);
  assert.ok(start >= 0, name);
  let i = src.indexOf('=>', start), depth = 0;
  for (; i < src.length; i++) {
    const ch = src[i];
    if (ch === '{' || ch === '(') depth++;
    if (ch === '}' || ch === ')') depth--;
    if (ch === ';' && depth === 0) break;
  }
  return src.slice(start, i + 1);
}
const ghCrashUrl = new Function(`${arrow(app, 'ghCrashUrl')} return ghCrashUrl;`)();
const parts = (url) => {
  const u = new URL(url);
  return { base: u.origin + u.pathname, title: u.searchParams.get('title'), body: u.searchParams.get('body') };
};
const crash = (lines, extra = {}) => ({
  reason: 'A Game closed with code 3221225477: access violation', note: '', version: '0.90.0-beta.40',
  os: 'Windows 11 (x86_64)', gpu: 'Some Graphics Card', game: 'A Game v1.0 (a-game)', exit: 'Closed with code 3221225477 after 12 s',
  file: 'C:\\Users\\someone\\Games\\a-game\\log.txt', text: lines.join('\n'), ...extra,
});

test('a crash report on GitHub has the versions, the PC, the game and the last 60 log lines', () => {
  const lines = Array.from({ length: 100 }, (_, i) => `line ${i + 1}`);
  const { base, title, body } = parts(ghCrashUrl(crash(lines)));
  assert.equal(base, 'https://github.com/elripalda/RecompLauncher/issues/new');
  assert.match(title, /^Crash: A Game closed with code/);
  for (const s of ['0.90.0-beta.40', 'Windows 11', 'Some Graphics Card', 'A Game v1.0 (a-game)', 'after 12 s']) assert.ok(body.includes(s), s);
  assert.match(body, /Last 60 lines of ~\\Games\\a-game\\log\.txt:\n```text\nline 41\n/);
  assert.match(body, /line 100\n```\n/);
  assert.ok(!body.includes('line 40\n'));
  assert.match(body, /attach the full log/);
  // A player's name in a folder stays out of a public issue.
  assert.ok(!body.includes('someone'));
});

test('a crash report address stays under 8 KB by dropping log lines from the top', () => {
  const lines = Array.from({ length: 60 }, (_, i) => `${i} ${'x'.repeat(300)} ä ✓`);
  const url = ghCrashUrl(crash(lines));
  assert.ok(url.length <= 8000, url.length);
  const { body } = parts(url);
  assert.match(body, /59 x+/, 'the last line stays');
  assert.ok(!body.includes('\n0 x'), 'the first goes');
  // One huge line keeps its end, and no log at all still makes a report.
  assert.ok(ghCrashUrl(crash(['y'.repeat(50000) + 'END'])).length <= 8000);
  assert.ok(parts(ghCrashUrl(crash(['y'.repeat(50000) + 'END']))).body.includes('END'));
  assert.match(parts(ghCrashUrl(crash([], { file: null, text: null }))).body, /RecompLauncher: 0\.90\.0-beta\.40/);
  // A log with its own code fence can't end the excerpt early.
  assert.match(parts(ghCrashUrl(crash(['a', '```', 'b']))).body, /````text\na\n```\nb\n````/);
});

test('the crash window has a Report on GitHub button that asks for the graphics card', () => {
  assert.match(app, /data-x="ghreport">\$\{esc\(t\('Report on GitHub'\)\)\}/);
  assert.match(app, /x\.dataset\.x === 'ghreport'/);
  assert.match(app, /invoke\('pc_specs'\)/);
});

test('the window opens even when a hidden window gets no frames', () => {
  assert.match(app, /requestAnimationFrame\(\(\) => requestAnimationFrame\(ready\)\);\r?\n\s*setTimeout\(ready, 150\);/);
});

test('light themes get their own grain so gradients stay smooth', () => {
  assert.match(css, /body\.light\{--noise:url\(img\/noise-light\.png\)/);
  assert.ok(fs.existsSync(path.join(__dirname, '../ui/img/noise-light.png')));
});
