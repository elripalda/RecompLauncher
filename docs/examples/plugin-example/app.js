// Game Notes: a small plugin that shows what a plugin can do. The `rl` object comes from
// /_sdk/rl.js, which the launcher serves to every plugin page.
const $ = (s) => document.querySelector(s);
let games = [];
let current = null;

async function load() {
  games = await rl.library.list(); // needs "library.read"
  games.sort((a, b) => a.name.localeCompare(b.name));
  $('#games').innerHTML = '';
  for (const g of games) {
    const b = document.createElement('button');
    b.textContent = g.name;
    b.onclick = () => pick(g.id);
    b.dataset.id = g.id;
    $('#games').append(b);
  }
  if (!games.length) $('#games').textContent = 'Your Library is empty.';
}

async function pick(id) {
  current = games.find((g) => g.id === id) || { id, name: id };
  for (const b of document.querySelectorAll('#games button')) b.classList.toggle('on', b.dataset.id === id);
  $('#editor').hidden = false;
  $('#title').textContent = current.name;
  $('#note').value = (await rl.storage.get('note:' + id)) || ''; // every plugin can keep its own data
  $('#note').focus();
}

$('#save').onclick = async () => {
  if (!current) return;
  await rl.storage.set('note:' + current.id, $('#note').value);
  rl.toast('Saved.');
};
$('#open').onclick = () => current && rl.openGame(current.id);

// A request to a site the plugin lists. The launcher asks the player the first time.
$('#ping').onclick = async () => {
  try {
    const r = await rl.net.fetch('https://example.com/');
    $('#out').textContent = 'example.com answered ' + r.status + '.';
  } catch (e) {
    $('#out').textContent = e.message;
  }
};

// The page is walled off from the launcher: none of these should be reachable.
$('#check').onclick = async () => {
  const seen = [];
  if (window.__TAURI_INTERNALS__ || window.__TAURI__) seen.push('launcher internals');
  if (window.ipc) seen.push('ipc');
  if (window.chrome && window.chrome.webview) seen.push('webview messaging');
  try { void parent.document.title; seen.push('the launcher page'); } catch (e) { /* walled off */ }
  try { await fetch('https://example.com/'); seen.push('the internet directly'); } catch (e) { /* walled off */ }
  $('#out').textContent = seen.length ? 'Reachable: ' + seen.join(', ') : 'Walled off: nothing of the launcher is reachable.';
};

// A game's right-click menu → Write a Note.
rl.on('menu', (d) => { if (d.item === 'note') pick(d.game.id); });

load();
