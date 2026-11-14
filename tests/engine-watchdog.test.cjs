const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawn } = require('node:child_process');

test('preparation stops when its launcher process disappears', { skip: process.platform === 'win32', timeout: 20000 }, async (t) => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(),'rl-watchdog-'));
  const guard = fs.readFileSync(path.join(__dirname,'../src-tauri/src/engines/adapters/nxnative/watch_parent.py'),'utf8');
  const builder = path.join(root,'builder.py');
  // Startup may take longer than the old 600 ms owner lifetime on a busy CI runner.
  fs.writeFileSync(builder,"import pathlib, time\ntime.sleep(0.8)\npathlib.Path('started').touch()\ntime.sleep(2)\npathlib.Path('survived').touch()\n");
  const script = `
    const { spawn } = require('node:child_process');
    const fs = require('node:fs');
    const path = require('node:path');
    const root = process.argv[3];
    const child = spawn('/usr/bin/python3', ['-c', process.argv[1], process.argv[2]], {
      cwd: root, detached: true, stdio: ['ignore', 'inherit', 'inherit']
    });
    child.on('error', error => { console.error(error); process.exit(1); });
    if (child.pid) fs.writeFileSync(path.join(root, 'pid'), String(child.pid));
    setInterval(() => { if (fs.existsSync(path.join(root, 'started'))) process.exit(0); }, 20);
    setTimeout(() => { console.error('Builder did not start'); process.exit(1); }, 10000);
  `;
  const owner = spawn(process.execPath,['-e',script,guard,builder,root]);
  let closed = false;
  let stderr = '';
  owner.stdout.resume();
  owner.stderr.on('data', chunk => { stderr += chunk; });
  t.after(() => {
    if (!closed) {
      owner.kill('SIGKILL');
      const pidFile = path.join(root, 'pid');
      if (fs.existsSync(pidFile)) {
        try { process.kill(-Number(fs.readFileSync(pidFile, 'utf8')), 'SIGKILL'); }
        catch (error) { if (error.code !== 'ESRCH') throw error; }
      }
    }
    fs.rmSync(root, { recursive: true, force: true });
  });
  // The builder inherits these pipes, so close waits for it too, not only its owner.
  const code = await new Promise((resolve,reject)=>{
    owner.on('error',reject);
    owner.on('close', code => { closed = true; resolve(code); });
  });
  assert.equal(code, 0, stderr);
  assert.ok(fs.existsSync(path.join(root,'started')),'the fixture actually started');
  assert.equal(fs.existsSync(path.join(root,'survived')),false);
});
