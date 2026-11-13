const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');

// A `const name = ...;` out of app.js, up to the line that closes it.
function konst(name) {
  const start = app.indexOf(`const ${name} = `);
  assert.ok(start >= 0, name);
  const oneLine = app.indexOf('\n', start);
  const first = app.slice(start, oneLine);
  if (!first.trimEnd().endsWith('{')) return first;
  const end = app.indexOf('\n};', start);
  return app.slice(start, end + 3);
}

// A function declaration out of app.js, by name.
function fn(name) {
  const start = app.indexOf(`function ${name}(`);
  assert.ok(start >= 0, name);
  let i = app.indexOf('{', start), depth = 0;
  for (; i < app.length; i++) {
    if (app[i] === '{') depth++;
    if (app[i] === '}' && --depth === 0) break;
  }
  return app.slice(start, i + 1);
}

const lib = new Function(`${konst('libMerge')} ${konst('libPending')} return { libMerge, libPending };`)();
const search = new Function(`${konst('fold')} ${konst('sfold')} ${konst('sMatch')} return sMatch;`)();
const hosts = new Function('t', 'tf', `${konst('isLab')} ${konst('isSF')} ${konst('isFJ')} ${konst('sfName')} ${konst('fjPath')} ${konst('fjServer')} ${konst('hostName')} ${konst('repoHome')} ${konst('repoIssues')} ${konst('repoReadme')} ${konst('repoFiles')} ${konst('viewOn')}
  return { isFJ, hostName, repoHome, repoIssues, repoReadme, repoFiles, viewOn };`)((k) => k, (k, o) => k.replace(/\{(\w+)\}/g, (_, n) => o[n]));
const verText = new Function(`${konst('verText')} return verText;`)();

test('a game added while the account list is being read stays in the Library', () => {
  const remote = { a: 100 };
  const queue = [{ id: 'b', on: true }];
  const pending = lib.libPending(remote, queue);
  assert.deepEqual(pending, [{ id: 'b', on: true }]);
  const next = lib.libMerge(remote, pending, { b: 200 });
  assert.deepEqual(next, { a: 100, b: 200 });
});

test('a queued change the account list already shows is no longer pending', () => {
  assert.deepEqual(lib.libPending({ a: 1, b: 2 }, [{ id: 'b', on: true }, { id: 'c', on: false }]), []);
  assert.deepEqual(lib.libPending({ a: 1 }, [{ id: 'a', on: false }]), [{ id: 'a', on: false }]);
});

test('search finds a name with its punctuation and spaces left out', () => {
  assert.ok(search('Sky-Racer!! Deluxe', 'sky racer'));
  assert.ok(search('Sky-Racer!! Deluxe', 'skyracer'));
  assert.ok(search('Café Quest', 'cafe quest'));
  assert.ok(!search('Sky-Racer!! Deluxe', 'cave quest'));
  assert.ok(search('anything', '  '));
});

test('a Forgejo project links to its own server, named by the server', () => {
  const repo = 'forgejo:git.example.dev/Studio/Hopper';
  assert.ok(hosts.isFJ(repo));
  assert.equal(hosts.hostName(repo), 'git.example.dev');
  assert.equal(hosts.repoHome(repo), 'https://git.example.dev/Studio/Hopper');
  assert.equal(hosts.repoFiles(repo), 'https://git.example.dev/Studio/Hopper/releases');
  assert.equal(hosts.repoIssues(repo), 'https://git.example.dev/Studio/Hopper/issues');
  assert.equal(hosts.repoReadme(repo), 'https://git.example.dev/Studio/Hopper');
  assert.equal(hosts.viewOn(repo), 'View on git.example.dev');
});

test('GitHub, GitLab and SourceForge links are as before', () => {
  assert.equal(hosts.repoHome('dev/game'), 'https://github.com/dev/game');
  assert.equal(hosts.repoIssues('dev/game'), 'https://github.com/dev/game/issues');
  assert.equal(hosts.viewOn('dev/game'), 'GitHub');
  assert.equal(hosts.repoHome('gitlab:group/game'), 'https://gitlab.com/group/game');
  assert.equal(hosts.repoIssues('gitlab:group/game'), 'https://gitlab.com/group/game/-/issues/new');
  assert.equal(hosts.viewOn('gitlab:group/game'), 'GitLab');
  assert.equal(hosts.repoFiles('sourceforge:gamebox'), 'https://sourceforge.net/projects/gamebox/files/');
  assert.equal(hosts.viewOn('sourceforge:gamebox'), 'View on SourceForge');
  assert.ok(!hosts.isFJ('dev/game') && !hosts.isFJ('gitlab:group/game') && !hosts.isFJ('forgejo:git.example.dev/Studio'));
});

test('a build name shows as it is, a version number with a v', () => {
  assert.equal(verText('1.2.0'), 'v1.2.0');
  assert.equal(verText('v1.2.0'), 'v1.2.0');
  assert.equal(verText('nightly-80-7315957c'), 'nightly-80-7315957c');
  assert.equal(verText(''), '');
});

test('a game brought in from a folder where it plays never asks for its files again', () => {
  const run = new Function('S', 'g', `${fn('needsCopySetup')} return needsCopySetup(g);`);
  const S = { setup: {}, easy: new Set(['a']) };
  assert.equal(run(S, { id: 'a' }), true);
  assert.equal(run(S, { id: 'a', files_in_place: true }), false);
  assert.equal(run(S, { id: 'a', sessions: [[0, 300]] }), false);
  assert.equal(run(S, { id: 'a', sessions: [[0, 30]] }), true);
});

// Just enough of an element for morphNode: attributes, and classes kept in the class attribute.
function el(tag, attrs) {
  const a = new Map(Object.entries(attrs));
  const classes = () => (a.get('class') || '').split(' ').filter(Boolean);
  return {
    nodeType: 1, tagName: tag, childNodes: [],
    get attributes() { return [...a].map(([name, value]) => ({ name, value })); },
    getAttribute: (k) => (a.has(k) ? a.get(k) : null),
    hasAttribute: (k) => a.has(k),
    setAttribute: (k, v) => a.set(k, String(v)),
    removeAttribute: (k) => a.delete(k),
    classList: {
      contains: (c) => classes().includes(c),
      add: (c) => a.set('class', [...classes(), c].join(' ')),
    },
  };
}
const morphNode = new Function('morphKids', `${fn('morphNode')} return morphNode;`)(() => {});

test('a redraw keeps a picture that already showed shown', () => {
  const shown = el('IMG', { src: 'cover.png', class: 'ld' });
  morphNode(shown, el('IMG', { src: 'cover.png', loading: 'lazy' }));
  assert.ok(shown.classList.contains('ld'));
  assert.equal(shown.getAttribute('loading'), 'lazy');
});

test('a redraw that changes the picture waits for the new one to load', () => {
  const swapped = el('IMG', { src: 'old.png', class: 'ld' });
  morphNode(swapped, el('IMG', { src: 'new.png' }));
  assert.ok(!swapped.classList.contains('ld'));
  assert.equal(swapped.getAttribute('src'), 'new.png');
});
