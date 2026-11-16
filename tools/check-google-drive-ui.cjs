// Exercises the shipped desktop and Big Picture UI with simulated Tauri responses. No Google account is contacted.
const assert = require('node:assert/strict');
const { spawn, spawnSync } = require('node:child_process');
const { createServer } = require('node:http');
const { readFile, mkdir, mkdtemp, writeFile } = require('node:fs/promises');
const { resolve, extname } = require('node:path');

const { tmpdir } = require('node:os');
const root = resolve(__dirname, '../ui');
const browser = process.env.RL_UI_CHROME || 'google-chrome-stable';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
(async () => {
  if (spawnSync(browser, ['--version']).status !== 0) throw new Error('Set RL_UI_CHROME to a Chrome executable to run this browser smoke check.');
  const output = process.argv[2] ? resolve(process.argv[2]) : await mkdtemp(resolve(tmpdir(), 'rl-drive-ui-'));
  await mkdir(output, { recursive: true });
  const server = createServer(async (req, res) => {
    const path = resolve(root, '.' + new URL(req.url, 'http://localhost').pathname);
    if (!path.startsWith(root + '/') && path !== root) { res.writeHead(403).end(); return; }
    try {
      const file = path === root ? path + '/index.html' : path;
      const mime = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.png': 'image/png', '.woff2': 'font/woff2' };
      res.setHeader('Content-Type', mime[extname(file)] || 'application/octet-stream');
      res.end(await readFile(file));
    } catch (_) { res.writeHead(404).end(); }
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  const chrome = spawn(browser, ['--headless=new', '--no-sandbox', '--disable-gpu', '--no-first-run', '--disable-background-networking', '--disable-crash-reporter', `--user-data-dir=${output}/profile`, '--remote-debugging-pipe', 'about:blank'], {
    stdio: ['ignore', 'ignore', 'pipe', 'pipe', 'pipe'],
    env: { ...process.env, XDG_CONFIG_HOME: output + '/config', XDG_CACHE_HOME: output + '/cache' },
  });
  let next = 0, bytes = '', session;
  const pending = new Map(), errors = [];
  chrome.stderr.on('data', (data) => writeFile(output + '/chrome.log', data, { flag: 'a' }));
  chrome.stdio[4].on('data', (data) => {
    bytes += data;
    let end;
    while ((end = bytes.indexOf('\0')) >= 0) {
      const message = JSON.parse(bytes.slice(0, end)); bytes = bytes.slice(end + 1);
      if (message.id) {
        const waiter = pending.get(message.id);
        if (waiter) { clearTimeout(waiter.timer); pending.delete(message.id); message.error ? waiter.reject(message.error) : waiter.resolve(message.result); }
      }
      if (message.method === 'Runtime.exceptionThrown') errors.push(message.params.exceptionDetails);
    }
  });
  const send = (method, params = {}, sessionId = session) => new Promise((resolve, reject) => {
    const id = ++next;
    const timer = setTimeout(() => { pending.delete(id); reject(new Error('Timed out: ' + method)); }, 10000);
    pending.set(id, { resolve, reject, timer });
    chrome.stdio[3].write(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }) + '\0');
  });
  const evaluate = async (expression) => {
    const result = await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
    if (result.exceptionDetails) throw new Error(JSON.stringify(result.exceptionDetails));
    return result.result.value;
  };
  try {
    const target = await send('Target.createTarget', { url: 'about:blank' });
    session = (await send('Target.attachToTarget', { targetId: target.targetId, flatten: true })).sessionId;
    await send('Page.enable'); await send('Runtime.enable');
    await send('Emulation.setDeviceMetricsOverride', { width: 1440, height: 1000, deviceScaleFactor: 1, mobile: false });
    await send('Page.addScriptToEvaluateOnNewDocument', { source: `
      window.smokeCalls = [];
      window.smokeState = { provider: 'recompstore', name: 'RecompStore', connected: false, google: { configured: true, deviceConfigured: false, connected: false } };
      window.smokeInstalled = {};
      const no = async () => {};
      const win = new Proxy({}, { get: (_, name) => name === 'isMaximized' ? async () => false : no });
      window.__TAURI__ = { core: { convertFileSrc: (u) => u, invoke: async (command, args) => {
        window.smokeCalls.push([command, args]);
        if (command === 'get_state') return { os: 'linux', arch: 'x64', version: '0.90.0-beta.53', site: '', library: window.smokeInstalled, settings: { setup_done: true, data_asked: true, check_updates: false, ui: { lang: 'en', reduce_motion: true, share_usage: false } } };
        if (command === 'get_catalog') return { data: { apps: [], origins: [] } };
        if (command === 'save_provider_state') return { state: window.smokeState, installed: window.smokeInstalled };
        if (command === 'account_state') return { signedIn: false };
        if (command === 'drive_connect_start') return { url: 'https://accounts.google.com/', device: false, expires: 600 };
        if (command === 'drive_connect_poll') return { status: 'pending' };
        if (['easy_setup', 'libraries', 'launcher_notes', 'team_notes', 'trending', 'game_sizes'].includes(command)) return [];
        if (['take_link', 'take_setup_asked', 'launcher_update', 'pc_check', 'news', 'ig_info'].includes(command)) return null;
        if (command === 'is_handheld') return false;
        if (command === 'plugin_list') return { plugins: [], enabled: [] };
        if (command === 'ach_status') return { tester: false };
        return {};
      } }, event: { listen: async () => no }, window: { getCurrentWindow: () => win }, dpi: { PhysicalPosition: class {}, PhysicalSize: class {} } };
    ` });
    await send('Page.navigate', { url: `http://127.0.0.1:${server.address().port}/index.html` });
    await sleep(1300);
    await evaluate("S.settings.setup_done = true; S.settings.data_asked = true; S.setTab = 'account'; go('settings');");
    await sleep(350);
    const desktop = await evaluate("({ text: document.querySelector('#main').innerText, connect: !!document.querySelector('[data-act=acc-drive-connect]:not(:disabled)'), calls: window.smokeCalls })");
    assert(desktop.connect, 'desktop connection button enabled');
    assert(/save destination/i.test(desktop.text), 'save destination settings rendered');
    await evaluate("document.querySelector('[data-act=acc-drive-connect]').click()");
    await sleep(100);
    assert(await evaluate("!!document.querySelector('[data-x=cancel]')"), 'connection dialog opens');
    await evaluate("document.querySelector('[data-x=cancel]').click()");
    await sleep(100);
    assert(await evaluate("window.smokeCalls.some(([command]) => command === 'drive_connect_cancel')"), 'closing connection dialog cancels sign-in');
    await evaluate("window.smokeState.google.configured = false; applySaveProvider({state:window.smokeState,installed:{}}); renderAccountBits();");
    assert(await evaluate("document.querySelector('[data-act=acc-drive-connect]').disabled"), 'unconfigured builds disable Google sign-in');
    await evaluate("window.smokeState.google.configured = true; applySaveProvider({state:window.smokeState,installed:{}}); renderAccountBits();");
    await writeFile(output + '/desktop-settings.webp', Buffer.from((await send('Page.captureScreenshot', { format: 'webp', quality: 82 })).data, 'base64'));
    await evaluate("window.smokeState = {provider:'google_drive',name:'Google Drive',connected:true,google:{configured:true,connected:true,email:'player@example.com'}}; window.smokeInstalled = {}; applySaveProvider({state:window.smokeState,installed:window.smokeInstalled}); renderAccountBits();");
    await sleep(200);
    const connected = await evaluate("({ disconnect: !!document.querySelector('[data-act=acc-drive-disconnect]'), text:document.querySelector('#main').innerText })");
    assert(connected.disconnect, 'connected account has disconnect control');
    assert(connected.text.includes('player@example.com'), 'connected account displayed');
    await evaluate("window.startTV();");
    await sleep(4000);
    await evaluate("window.tvGoto('settings');");
    await sleep(650);
    const tv = await evaluate("({ text:document.querySelector('#tv').innerText, provider:!!document.querySelector('[data-tvact=save-provider]'), disconnect:!!document.querySelector('[data-tvact=drive-disconnect]') })");
    await writeFile(output + '/tv-debug.json', JSON.stringify({tv,errors,calls:await evaluate('window.smokeCalls')},null,2));
    assert(tv.provider && tv.disconnect, 'Big Picture provider and disconnect controls rendered');
    await send('Input.dispatchKeyEvent', { type: 'keyDown', key: 'ArrowDown', code: 'ArrowDown', windowsVirtualKeyCode: 40 });
    await send('Input.dispatchKeyEvent', { type: 'keyUp', key: 'ArrowDown', code: 'ArrowDown', windowsVirtualKeyCode: 40 });
    await sleep(350);
    assert(await evaluate("document.querySelector('[data-tvact=save-provider]').classList.contains('f')"), 'keyboard navigation reaches the save provider control');
    await writeFile(output + '/big-picture-settings.webp', Buffer.from((await send('Page.captureScreenshot', { format: 'webp', quality: 82 })).data, 'base64'));
    await writeFile(output + '/results.json', JSON.stringify({ desktop, connected, tv, errors }, null, 2));
    console.log(JSON.stringify({ desktop: 'passed', connected: 'passed', bigPicture: 'passed', errors: errors.map((e) => e.exception?.description || e.text), output }));
    if (errors.length) process.exitCode = 1;
  } finally {
    chrome.kill('SIGTERM'); server.close();
  }
})().catch((error) => { console.error(error); process.exitCode = 1; });
