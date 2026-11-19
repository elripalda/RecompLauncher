const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const start = app.indexOf('// Playing with others: how (online, on this network)');
const end = app.indexOf('\n}\n', app.indexOf('function networkCard(')) + 3;
assert.ok(start >= 0 && end > start);

function card(network) {
  const ctx = vm.createContext({
    esc: (s) => String(s),
    t: (s) => s,
    tf: (s, o) => s.replace(/\{(\w+)\}/g, (_, k) => o[k]),
  });
  vm.runInContext(app.slice(start, end), ctx);
  return ctx.networkCard('game', network);
}

const server = { label: 'Dedicated server', port: 27015 };

test('a game without a network block shows no card', () => {
  assert.equal(card(null), '');
});

test('online and LAN modes are both named', () => {
  assert.match(card({ modes: ['online', 'lan'], server: null }), /Online and on your local network/);
});

test('a game with only modes shows them without Host a Server', () => {
  const html = card({ modes: ['lan'], server: null });
  assert.match(html, /On your local network/);
  assert.doesNotMatch(html, /srv-start/);
});

test('a server that is not running offers Host a Server on its port', () => {
  const html = card({ modes: ['lan'], server, hosting: null });
  assert.match(html, /data-act="srv-start"/);
  assert.match(html, /value="27015"/);
});

test('a running server shows the address to share and Stop', () => {
  const html = card({ modes: ['lan'], server, hosting: { port: 27015, address: '192.168.0.10:27015' } });
  assert.match(html, /join 192\.168\.0\.10:27015/);
  assert.match(html, /data-act="srv-stop"/);
  assert.match(html, /data-act="srv-copy"/);
});

test('a running server on a PC with no network says only it can join', () => {
  const html = card({ modes: ['lan'], server, hosting: { port: 27015, address: null } });
  assert.match(html, /only it can join/);
  assert.doesNotMatch(html, /srv-copy/);
});
