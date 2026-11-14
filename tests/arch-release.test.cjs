const { test } = require('node:test');
const assert = require('node:assert/strict');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

function checkRelease(tag, packageName) {
  const env = { ...process.env, GH_TOKEN: '', GITHUB_REPOSITORY: '' };
  return spawnSync('bash', [path.join(__dirname, '../tools/check-release-files.sh'), tag], {
    input: `RecompLauncher_${tag.slice(1)}_x64-setup.exe sha256:installer\n${packageName} sha256:package\n`,
    encoding: 'utf8', env,
  });
}

test('release accepts an Arch prerelease with makepkg version spelling', { skip: process.platform !== 'linux' }, () => {
  const result = checkRelease('v0.90.0-beta.42', 'recomplauncher-0.90.0beta.42-1-x86_64.pkg.tar.zst');
  assert.equal(result.status, 0, result.stdout + result.stderr);
});

test('release rejects an Arch package from another beta', { skip: process.platform !== 'linux' }, () => {
  const result = checkRelease('v0.90.0-beta.42', 'recomplauncher-0.90.0beta.41-1-x86_64.pkg.tar.zst');
  assert.equal(result.status, 1, result.stdout + result.stderr);
});

test('release rejects a stable Arch package on a beta tag', { skip: process.platform !== 'linux' }, () => {
  const result = checkRelease('v0.90.0-beta.42', 'recomplauncher-0.90.0-1-x86_64.pkg.tar.zst');
  assert.equal(result.status, 1, result.stdout + result.stderr);
});

test('release accepts a stable Arch package and a packaging revision', { skip: process.platform !== 'linux' }, () => {
  const result = checkRelease('v0.90.0', 'recomplauncher-0.90.0-2-x86_64.pkg.tar.zst');
  assert.equal(result.status, 0, result.stdout + result.stderr);
});
