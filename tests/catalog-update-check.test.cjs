const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');

function catalogFreshSource() {
  const from = app.indexOf("T.event.listen('catalog-fresh'");
  assert.ok(from >= 0);
  const line = app.slice(from, app.indexOf('\n', from));
  if (!line.endsWith('{')) return line;
  return app.slice(from, app.indexOf('\n});', from) + 4);
}

function freshCatalog({ before, after, checkUpdates = true }) {
  let handler;
  let checks = 0;
  let tag = before;
  const ctx = vm.createContext({
    S: { settings: { check_updates: checkUpdates } },
    T: { event: { listen: (_name, fn) => { handler = fn; } } },
    approvedReleases: () => tag,
    loadCatalog: async () => { tag = after; },
    render: () => {},
    checkUpdates: async () => { checks++; },
    updateCheckRevision: 0,
  });
  vm.runInContext(catalogFreshSource(), ctx);
  return { run: () => handler(), get checks() { return checks; } };
}

test('a newly approved release is checked when the fresh catalog arrives', async () => {
  const site = freshCatalog({ before: 'v1', after: 'v2' });
  await site.run();
  assert.equal(site.checks, 1);
});

test('a fresh catalog with the same releases asks nothing more', async () => {
  const site = freshCatalog({ before: 'v1', after: 'v1' });
  await site.run();
  assert.equal(site.checks, 0);
});

test('a fresh catalog respects update checks being turned off', async () => {
  const site = freshCatalog({ before: 'v1', after: 'v2', checkUpdates: false });
  await site.run();
  assert.equal(site.checks, 0);
});
