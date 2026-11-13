const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const tv = fs.readFileSync(path.join(__dirname, '../ui/tv.js'), 'utf8');
const fn = (src, name) => src.match(new RegExp(`^  (?:async )?function ${name}\\([^]*?^  }`, 'm'))[0];

function downloadsContext(launcherUpd) {
  const c = vm.createContext({
    S: { jobs: {}, recent: [], updates: {}, library: {}, dlHist: [], launcherUpd, version: '0.90.0-beta.39' },
    TV: {},
    t: (s) => s, esc: String,
    tf: (s, values) => s.replace(/\{(\w+)\}/g, (_, k) => values[k]),
    verName: (v) => v,
    backdrop: () => '', bar: () => '', region: (s) => s, pageHead: (s) => s, hints: () => '',
    libAppOf: (id) => ({ id, name: id }), media: String, scanInfo: () => null, progress: () => '', cleanMsg: String,
    sfx: () => {}, calls: [],
  });
  c.selfUpdate = () => c.calls.push('selfUpdate');
  vm.runInContext(fn(tv, 'pageDownloads'), c);
  vm.runInContext(fn(tv, 'activate'), c);
  return c;
}

test('Big Picture Downloads lists the launcher update the badge counts', () => {
  const html = downloadsContext({ version: '0.90.0-beta.40' }).pageDownloads();
  assert.match(html, /data-key="u:@launcher" data-tvact="selfupdate"><img class="mark" src="img\/mark.svg"/);
  assert.match(html, /RecompLauncher 0.90.0-beta.40 is ready<small>You have 0.90.0-beta.39\./);
  assert.doesNotMatch(html, /Nothing downloading\./);
});

test('Big Picture Downloads has no launcher row without a launcher update', () => {
  const html = downloadsContext(null).pageDownloads();
  assert.doesNotMatch(html, /u:@launcher/);
  assert.match(html, /Nothing downloading\./);
});

test('pressing A on the launcher row starts the same selfUpdate as the desktop', async () => {
  const c = downloadsContext({ version: '0.90.0-beta.40' });
  await c.activate({ dataset: { tvact: 'selfupdate', key: 'u:@launcher' }, closest: () => null });
  assert.deepEqual(c.calls, ['selfUpdate']);
});
