// setup-inputs.js and setup-steps.js (Easy Setup's steps) in a sandbox, with the little the page
// gives them. owned: what the folder scan found, by game id.
const vm = require('node:vm');
const fs = require('node:fs');
const path = require('node:path');
const read = (f) => fs.readFileSync(path.join(__dirname, '../ui', f), 'utf8');

function stepsContext(owned = {}, extra = {}) {
  const tf = (s, v) => s.replace(/\{(\w+)\}/g, (m, k) => (k in v ? v[k] : m));
  const ctx = vm.createContext(Object.assign({
    t: (x) => x, tf, tn: (n, one, many) => tf(n === 1 ? one : many, { n }),
    esc: (x) => String(x ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c])),
    htmlFill: (s, v) => s.replace(/\{(\w+)\}/g, (m, k) => (k in v ? v[k] : m)),
    baseName: (x) => String(x).split(/[\\/]/).pop(), CHECK: '<svg class="ok"></svg>', XMARK: '<svg></svg>', DISC_L: '<svg></svg>', DISC_S: '<svg></svg>',
    S: { owned, library: {}, setup: {} },
    copyFormats: () => ({ type: 'disc', exts: ['iso'], folder: true }), copyDrop: () => 'Drop it', copyPickOpts: () => ({}),
    prepReq: () => 'Turn on the option marked Required to continue',
    prepBlocked: (su) => (su.prepare_options || []).some((o) => o.required && !o.on),
    gameMediaHtml: () => '<div class="md"></div>',
  }, extra));
  vm.runInContext(read('setup-inputs.js'), ctx);
  vm.runInContext(read('setup-steps.js'), ctx);
  return ctx;
}

module.exports = { stepsContext, read };
