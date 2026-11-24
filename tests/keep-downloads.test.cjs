const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const fn = (src, name) => src.match(new RegExp(`^function ${name}\\([^]*?^}`, 'm'))[0];

// A Settings page (Updates and Downloads unless another tab is given), drawn with the given settings.
function updatesPage(settings, setTab = 'updates', libs = []) {
  const c = vm.createContext({
    S: {
      os: 'windows', settings, libs, library: {}, owned: {}, roms: [],
      ids: { usage: 'usage-id', stats: 'stats-id' }, setTab, view: 'settings',
    },
    t: (s) => s, N_: (s) => s, tc: (s) => s, esc: String,
    tf: (s, values) => s.replace(/\{(\w+)\}/g, (_, k) => values[k]),
    LANGS: [['en', 'English']], systemLang: () => 'en',
    SET_SECTIONS: [['updates', 'Updates'], ['library', 'Games and folders']], SET_ICONS: {}, DL_LIMITS: [0],
    okMark: () => '', portableRow: () => '', geRow: () => '', networkGroup: () => '', selfUpdatesGroup: () => '',
    backdrop: () => '', bar: () => '', region: (s) => s, pageHead: (s) => s,
    homePinned: () => false, ownedGames: () => [], bgMode: () => '', langName: () => 'English',
    svg: () => '', hints: () => '', HEART: '', setClock: () => {},
    autoInstall: () => false, autoUpdateAll: () => false,
    // What the Games and folders tab draws besides its libraries.
    tn: (n, one, many) => (n === 1 ? one : many.replace('{n}', n)), fmtB: String, folderCounts: () => '', sharedLibraryNote: () => '', dlssGroup: () => '', steamSyncGroup: () => '',
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

test('with Keep downloaded files on, every library has Kept Downloads for its own folder', () => {
  const libs = [{ path: '/home/me/Games', default: true, games: 1 }, { path: '/mnt/big/Games', games: 2 }];
  const html = updatesPage({ keep_downloads: true, library_dir: '/home/me/Games' }, 'library', libs);
  assert.match(html, /data-path="\/home\/me\/Games\/\.downloads">Kept Downloads</);
  assert.match(html, /data-path="\/mnt\/big\/Games\/\.downloads">Kept Downloads</);
});

test('with Keep downloaded files off, the libraries have no Kept Downloads', () => {
  const libs = [{ path: '/mnt/big/Games', default: true, games: 2 }];
  const html = updatesPage({ keep_downloads: false, library_dir: '/mnt/big/Games' }, 'library', libs);
  assert.doesNotMatch(html, /Kept Downloads/);
});
