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

test('a home folder under /var (Bazzite, Silverblue) becomes ~ as a whole', () => {
  const file = '/var/home/someone/.local/share/com.recomplauncher.app/game-output/g.log';
  const { body } = parts(ghCrashUrl(crash(['Error while copying to "/var/home/someone/.local/share/x/a.dll"'], { file })));
  assert.ok(body.includes('Last 1 lines of ~/.local/share/com.recomplauncher.app/game-output/g.log:'));
  assert.ok(body.includes('"~/.local/share/x/a.dll"'));
});

test('a Windows user folder seen from Linux becomes ~ as a whole', () => {
  const { body } = parts(ghCrashUrl(crash(['loading /mnt/c/Users/someone/AppData/game.bin'])));
  assert.ok(body.includes('loading ~/AppData/game.bin'));
});

// This PC's home folder as Rust hands it over (see home_forms in usage.rs).
const onLinux = { forms: ['Z:\\var\\home\\ruben', '/var/home/ruben', 'C:\\users\\ruben', 'Z:\\home\\ruben', '/home/ruben'], names: ['ruben'] };
const onWindows = { forms: ['\\\\?\\C:\\Users\\Ruben', 'C:\\Users\\Ruben'], names: ['Ruben'] };
const report = (lines, home, extra = {}) => parts(ghCrashUrl(crash(lines, { home, ...extra }))).body;

test('a Linux home folder goes from a crash report in every form a log shows it', () => {
  const body = report([
    'at /var/home/ruben/.local/share/x.so', 'at /home/ruben/Games/a.bin', 'open file:///home/ruben/x.html',
    'cc -I/home/ruben/inc', 'wine: Z:\\home\\ruben\\Games\\a.exe', 'wine: Z:\\var\\home\\ruben\\b.exe',
    'prefix C:\\users\\ruben\\AppData\\c.dll', 'mixed Z:/home/ruben\\d.dll',
  ], onLinux, { file: '/var/home/ruben/.local/share/g.log' });
  assert.ok(!/ruben/i.test(body), body);
  for (const s of ['Last 8 lines of ~/.local/share/g.log:', 'at ~/.local/share/x.so', 'at ~/Games/a.bin', 'open file://~/x.html', 'cc -I~/inc',
    'wine: ~\\Games\\a.exe', 'wine: ~\\b.exe', 'prefix ~\\AppData\\c.dll', 'mixed ~\\d.dll']) assert.ok(body.includes(s), s);
});

test('a Windows home folder goes in any case and in its long form', () => {
  const body = report(['load \\\\?\\C:\\Users\\Ruben\\AppData\\x.dll', 'load c:\\users\\ruben\\y.dll', 'load C:/Users/Ruben/z.dll', 'open file:///C:/Users/Ruben/x.html'], onWindows);
  assert.ok(!/ruben/i.test(body), body);
  for (const s of ['load ~\\AppData\\x.dll','load ~\\y.dll', 'load ~/z.dll', 'open file:///~/x.html', 'Last 4 lines of ~\\Games']) assert.ok(body.includes(s), s);
});

test('a removable drive under /run/media hides whose it is', () => {
  const body = report(['games on /run/media/ruben/SD/a.bin', 'and /run/media/deck/SD/b.bin', 'wine: Z:\\run\\media\\deck\\SD\\c.exe'], onLinux);
  for (const s of ['/run/media/~/SD/a.bin', '/run/media/~/SD/b.bin', 'Z:\\run\\media\\~\\SD\\c.exe']) assert.ok(body.includes(s), s);
});

test('the user name goes as a whole word, in any case, and common names stay', () => {
  assert.ok(report(['Hello Ruben, RUBEN signed in; rubenesque stays'], onLinux).includes('Hello ~, ~ signed in; rubenesque stays'));
  // A short name or a word a log uses anyway would change ordinary text.
  for (const n of ['deck', 'steam', 'user', 'admin', 'home', 'al']) {
    assert.ok(report([`the ${n} said hi`], { forms: [], names: [n] }).includes(`the ${n} said hi`), n);
  }
});

test('a path from another PC still loses its home folder by its shape', () => {
  const body = report([
    'a /var/home/someone/x', 'b Z:\\home\\someone\\x', 'c Z:\\var\\home\\someone\\x', 'd file:///home/someone/x',
    'e -I/home/someone/inc', 'f \\\\?\\C:\\Users\\someone\\x', 'g /Users/someone/x',
  ], onLinux);
  assert.ok(!body.includes('someone'), body);
});

test('web addresses in a crash report stay as they are', () => {
  const urls = ['https://github.com/Users/foo', 'https://example.com/home/ruben/page', 'http://host/run/media/ruben/x'];
  const body = report(urls.map((u) => `see ${u}`), onLinux);
  for (const u of urls) assert.ok(body.includes(`see ${u}`), u);
  // And without this PC's home folder too.
  assert.ok(report(['see https://github.com/Users/foo'], undefined).includes('see https://github.com/Users/foo'));
});
