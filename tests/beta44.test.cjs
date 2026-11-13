const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');

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

test('the Library adds a recomp by any project link', () => {
  assert.match(app, /data-act="gh-add">\$\{BRANCH_S\}\$\{esc\(t\('Add a Recomp'\)\)\}/);
  const modal = fn('githubModal');
  assert.match(modal, /<h2>\$\{esc\(t\('Add a Recomp'\)\)\}<\/h2>/);
  assert.match(modal, /Paste the link to the recomp\\'s project page\. GitHub, GitLab, SourceForge and Forgejo or Gitea servers all work\./);
  assert.match(modal, /placeholder="https:\/\/\.\.\."/);
  assert.doesNotMatch(modal, /GitHub or GitLab/);
});

test('the message for a link that can\'t be read is translated', () => {
  assert.match(app, /N_\('We couldn\\'t read that project\. Check the link, or make sure it has a release with your builds\.'\)/);
});

test('a project\'s own setup file arriving refreshes Easy Setup', () => {
  assert.match(app, /T\.event\.listen\('profiles-fresh', \(\) => loadCatalog\(false\)\.then\(\(\) => loadEasy\(true\)\)\.then\(\(\) => render\(true\)\)/);
});

test('the developer check names a setup file read from the project', () => {
  assert.match(app, /repo: t\('The recomplauncher\.json in its project'\)/);
});

test('a recomp request takes the whole link, with https:// added when it was left out', () => {
  const start = app.indexOf('const reqUrl = ');
  const end = app.indexOf('\n', start);
  const reqUrl = new Function(`${app.slice(start, end)} return reqUrl;`)();
  assert.equal(reqUrl(' git.example.dev/studio/game '), 'https://git.example.dev/studio/game');
  assert.equal(reqUrl('https://github.com/dev/project'), 'https://github.com/dev/project');
  assert.equal(reqUrl(''), '');
  assert.doesNotMatch(fn('requestModal'), /data-forge|rq-host/);
});
