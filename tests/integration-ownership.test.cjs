const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');

const source = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
// The Launcher Support box on Overview, for a catalog entry and (installed) its own manifest.
async function support(entry, local = null) {
  const ctx = vm.createContext({
    S: { library: local ? { game: {} } : {}, easy: new Set(), supLocal: local ? { game: local } : {} },
    invoke: async () => local, builtinBoot: () => ({}), TURNKEY_HOW: [],
    t: (s) => s, N_: (s) => s, tf: (s, args) => s.replace('{name}', args.name),
    esc: (s) => String(s).replaceAll('&', '&amp;').replaceAll('<', '&lt;'),
    okMark: (yes) => yes ? 'available' : 'unavailable',
    a: { id: 'game', name: 'Example Game', ...entry },
  });
  vm.runInContext(source.match(/^const OWNER_LONG = .*;$/m)[0], ctx);
  vm.runInContext(source.match(/^function supportCard\([^]*?^}/m)[0], ctx);
  return vm.runInContext('supportCard(a)', ctx);
}

test('an installed manifest provides capabilities without confirming developer maintenance', async () => {
  const html = await support({}, { settings: true, easy_setup: true });
  assert.match(html, /Nobody has confirmed who maintains it/);
  assert.match(html, /From Manifest/);
  assert.match(html, /Manifest found in the installed files/);
  assert.doesNotMatch(html, /This recomp supports RecompLauncher itself|By the Developer/);
});

test('release and repository manifests do not imply confirmed ownership', async () => {
  for (const sdk of ['release', 'repo']) {
    const html = await support({ support: { sdk, easy_setup: 'dev' } });
    assert.match(html, /Nobody has confirmed who maintains it/);
    assert.match(html, /From Manifest/);
    assert.doesNotMatch(html, /This recomp supports RecompLauncher itself|Maintained by its developer/);
  }
});

test('confirmed maintenance is separate from manifest and capability sources', async () => {
  const labels = { project: 'its developer', official: 'its developer', launcher: 'RecompLauncher', community: 'the community' };
  for (const [maintained_by, label] of Object.entries(labels)) {
    const html = await support({ maintained_by, support: { sdk: 'repo', settings: 'launcher' } });
    assert.ok(html.includes(`Maintained by ${label}`));
    assert.match(html, /By RecompLauncher/);
    assert.match(html, /Manifest in the project repository/);
  }
  assert.match(await support({ maintained_by: 'unexpected' }), /Nobody has confirmed who maintains it/);
});
