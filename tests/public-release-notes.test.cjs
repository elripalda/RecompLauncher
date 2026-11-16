const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { execFileSync } = require('node:child_process');

const root = path.join(__dirname, '..');
const renderer = path.join(root, 'tools/public-release-notes.py');
const python = process.platform === 'win32' ? 'python' : 'python3';
function render(text) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'recomp-release-notes-'));
  const file = path.join(dir, 'notes.md');
  fs.writeFileSync(file, text);
  try { return execFileSync(python, [renderer, file], { encoding: 'utf8' }); }
  finally { fs.rmSync(dir, { recursive: true }); }
}

test('the release body excludes SDK handoff content and retains player notes', () => {
  const draft = '# Release\n\n- A player feature.\n\n## For the website (SDK guide)\n\nPrivate SDK instructions.\n### Example\nMore private text.\n\n## Fixes\n\n- A player fix.\n';
  const publicNotes = render(draft);
  assert.match(publicNotes, /- A player feature\./);
  assert.match(publicNotes, /## Fixes\n\n- A player fix\./);
  assert.doesNotMatch(publicNotes, /SDK|Private|private|# Release/);
});

test('notes without a handoff keep their public content', () => {
  assert.equal(render('# Release\n\n- One fix.\n'), '- One fix.\n');
  assert.equal(render(''), '');
});

test('the Beta 53 release body leaves the SDK handoff in the source draft only', () => {
  const file = path.join(root, 'docs/PATCH-NOTES-0.90-beta-53.md');
  const before = fs.readFileSync(file, 'utf8');
  const body = execFileSync(python, [renderer, file], { encoding: 'utf8' });
  assert.match(before, /For the website \(SDK guide\)/);
  assert.match(body, /Easy Install lets you choose/);
  assert.doesNotMatch(body, /For the website|Explain that Easy Install|SDK guide/);
  assert.equal(fs.readFileSync(file, 'utf8'), before);
});

test('new and existing draft releases use the filtered body', () => {
  const workflow = fs.readFileSync(path.join(root, '.github/workflows/release.yml'), 'utf8');
  assert.match(workflow, /python3 tools\/public-release-notes\.py "\$notes" > body\.md/);
  assert.match(workflow, /gh release edit "\$TAG" --notes-file body\.md/);
  assert.match(workflow, /gh release create[^\n]+--notes-file body\.md/);
});
