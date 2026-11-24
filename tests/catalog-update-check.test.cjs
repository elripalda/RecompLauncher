const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8').replace(/\r\n/g, '\n');

function catalogFreshSource() {
  const from = app.indexOf("T.event.listen('catalog-fresh'");
  assert.ok(from >= 0);
  return app.slice(from, app.indexOf('\n});', from) + 4);
}

function afterCatalogSource() {
  const from = app.indexOf('function afterCatalog()');
  assert.ok(from >= 0);
  return app.slice(from, app.indexOf('\n}', from) + 2);
}

function freshCatalog({ before, after }) {
  let handler;
  let checks = 0;
  let tag = before;
  const ctx = vm.createContext({
    T: { event: { listen: (_name, fn) => { handler = fn; } } },
    approvedReleases: () => tag,
    loadCatalog: async () => { tag = after; },
    render: () => {},
    afterCatalog: async () => { checks++; },
  });
  vm.runInContext(catalogFreshSource(), ctx);
  return { run: () => handler(), get checks() { return checks; } };
}

function updateCheck({ checkUpdates = true }) {
  const ctx = vm.createContext({
    S: { settings: { check_updates: checkUpdates } },
    checks: 0,
    updateCheckRevision: 0,
  });
  vm.runInContext('function checkUpdates() { checks++; return Promise.resolve(true); }', ctx);
  vm.runInContext(afterCatalogSource(), ctx);
  return ctx;
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

test('the update check for a catalog retires any older check', async () => {
  const ctx = updateCheck({});
  await vm.runInContext('afterCatalog()', ctx);
  assert.equal(ctx.checks, 1);
  assert.equal(ctx.updateCheckRevision, 1);
});

test('the update check for a catalog respects update checks being turned off', async () => {
  const ctx = updateCheck({ checkUpdates: false });
  await vm.runInContext('afterCatalog()', ctx);
  assert.equal(ctx.checks, 0);
});

test('startup checks the saved catalog through the same function', () => {
  const catalogIn = app.indexOf('  await loadCatalog(false, catAsk);');
  const startupCheck = app.indexOf('  const upd = afterCatalog();');
  assert.ok(catalogIn >= 0);
  assert.ok(startupCheck > catalogIn);
});
