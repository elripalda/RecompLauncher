const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const fn = (src, name) => src.match(new RegExp(`^function ${name}\\([^]*?^}`, 'm'))[0];

// Settings → Updates and Downloads, drawn with the given settings.
function updatesPage(settings) {
  const c = vm.createContext({
    S: {
      os: 'windows', settings, libs: [], library: {}, owned: {},
      ids: { usage: 'usage-id', stats: 'stats-id' }, setTab: 'updates', view: 'settings',
    },
    t: (s) => s, N_: (s) => s, tc: (s) => s, esc: String,
    tf: (s, values) => s.replace(/\{(\w+)\}/g, (_, k) => values[k]),
    LANGS: [['en', 'English']], systemLang: () => 'en',
    SET_SECTIONS: [['updates', 'Updates']], SET_ICONS: {}, DL_LIMITS: [0],
    okMark: () => '', portableRow: () => '', geRow: () => '', networkGroup: () => '', selfUpdatesGroup: () => '',
    backdrop: () => '', bar: () => '', region: (s) => s, pageHead: (s) => s,
    homePinned: () => false, ownedGames: () => [], bgMode: () => '', langName: () => 'English',
    svg: () => '', hints: () => '', HEART: '', setClock: () => {},
    autoInstall: () => false, autoUpdateAll: () => false,
  });
  c.window = c;
  c.clockRow = () => ''; c.monitorPicked = () => ''; c.monitorName = () => '';
  vm.runInContext(fn(app, 'pageSettings'), c);
  return c.pageSettings();
}

test('with Keep downloaded files on, Open goes to the library\'s .downloads folder', () => {
  const html = updatesPage({ keep_downloads: true, library_dir: 'C:\\Recomp Library' });
  assert.match(html, /data-act="folder" data-path="C:\\Recomp Library\\\.downloads"/);
});

test('a Linux or Mac library gets its .downloads folder with a forward slash', () => {
  const html = updatesPage({ keep_downloads: true, library_dir: '/home/me/Games/Recomp Library/' });
  assert.match(html, /data-path="\/home\/me\/Games\/Recomp Library\/\.downloads"/);
});

test('with Keep downloaded files off, there is no Open for kept downloads', () => {
  const html = updatesPage({ keep_downloads: false, library_dir: 'C:\\Recomp Library' });
  assert.doesNotMatch(html, /\.downloads"/);
});
