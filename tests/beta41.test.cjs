const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');

// The Downloads graph's point keeper, run with a clock the test moves.
function speedKeeper() {
  const start = app.indexOf('const DLSPD = {};'), end = app.indexOf('window.dlSpeedSeed');
  assert.ok(start > 0 && end > start);
  const clock = { now: 1_000_000 };
  const run = new Function('Date', `${app.slice(start, end)} return { DLSPD, noteSpeed };`);
  return { clock, ...run({ now: () => clock.now }) };
}
// A download at a steady speed, with a progress update every `every` ms.
function download(k, secs, bytesPerSec, every = 150) {
  let done = 0;
  for (let t = 0; t < secs * 1000; t += every) {
    k.clock.now += every;
    done += bytesPerSec * every / 1000;
    k.noteSpeed('g', { done, total: 1e12, speed: 1 });
  }
  return k.DLSPD.g;
}

test('a 3-second download draws about five points a second', () => {
  const l = download(speedKeeper(), 3, 40e6);
  assert.ok(l.length >= 14 && l.length <= 16, String(l.length));
});

test('the graph keeps a minute and a half of points on a long download', () => {
  assert.equal(download(speedKeeper(), 120, 40e6).length, 450);
});

test('the speed comes from the bytes that arrived, not the slower number from the download', () => {
  const l = download(speedKeeper(), 5, 40e6);
  const last = l[l.length - 1];
  assert.ok(Math.abs(last - 40e6) / 40e6 < 0.05, String(last));
});

test('a retry that starts over never draws a negative or giant speed', () => {
  const k = speedKeeper();
  download(k, 2, 40e6);
  k.clock.now += 150;
  k.noteSpeed('g', { done: 1e6, total: 1e12, speed: 1 }); // back near the start
  for (let i = 0, done = 1e6; i < 10; i++) { k.clock.now += 150; done += 6e6; k.noteSpeed('g', { done, total: 1e12, speed: 1 }); }
  for (const v of k.DLSPD.g) assert.ok(v >= 0 && v < 100e6, String(v));
});

test('a Proton without its starting files gets Install GE-Proton in the crash window', () => {
  assert.match(app, /const pfxGone = S\.os === 'linux' && \/Default prefix is missing\/i\.test\(said\);/);
  assert.match(app, /const armProton = pfxGone \|\|/);
  assert.match(app, /armProton \? esc\(pfxGone \? t\('The Proton it ran through is missing the files/);
});

// Request a Recomp: GitHub or GitLab, then developer/project.
function reqLinkFn() {
  const start = app.indexOf('const reqLink = ');
  assert.ok(start > 0);
  const end = app.indexOf('\n};', start) + 3;
  return new Function(`${app.slice(start, end)} return reqLink;`)();
}

test('a request is built from the picked site and developer/project', () => {
  const reqLink = reqLinkFn();
  assert.deepEqual(reqLink('github', 'some-dev/some-project'), { forge: 'github', url: 'https://github.com/some-dev/some-project' });
  assert.deepEqual(reqLink('gitlab', 'group/sub/project'), { forge: 'gitlab', url: 'https://gitlab.com/group/sub/project' });
  assert.equal(reqLink('github', ' dev/project.git ').url, 'https://github.com/dev/project');
});

test('a whole link pasted in picks its own site', () => {
  const reqLink = reqLinkFn();
  assert.deepEqual(reqLink('github', 'https://gitlab.com/dev/project'), { forge: 'gitlab', url: 'https://gitlab.com/dev/project' });
  assert.equal(reqLink('gitlab', 'github.com/dev/project/').url, 'https://github.com/dev/project');
  assert.equal(reqLink('github', 'https://github.com/dev/project#readme').url, 'https://github.com/dev/project');
});

test('a request needs both a developer and a project', () => {
  const reqLink = reqLinkFn();
  for (const bad of ['', 'project', 'dev/', '/project', 'dev/../x', 'dev/pro!ject', 'a/b/c']) assert.equal(reqLink('github', bad).url, '', bad);
  assert.equal(reqLink('github', 'https://codeberg.org/dev/project').url, '');
});
