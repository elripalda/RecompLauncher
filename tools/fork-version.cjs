#!/usr/bin/env node
// Gives a fork build its own version, so the launcher's updater (which reads the fork's
// version.json) sees each fork build as newer than the one before, even on the same upstream
// version: upstream 0.90.0-beta.64 → 0.90.0-beta.64.<build>, upstream 1.2.0 → 1.2.0-fork.<build>.
// Used by .github/workflows/fork-sync.yml before building; never committed.
//
//   node tools/fork-version.cjs <build number>      prints the new version and writes it

const fs = require('node:fs');
const path = require('node:path');

const root = path.join(__dirname, '..');
const build = Number(process.argv[2]);
if (!Number.isInteger(build) || build < 1) {
  console.error('Usage: node tools/fork-version.cjs <build number>');
  process.exit(1);
}

const pkgFile = path.join(root, 'package.json');
const pkg = JSON.parse(fs.readFileSync(pkgFile, 'utf8'));
const upstream = pkg.version;
const version = upstream.includes('-') ? `${upstream}.${build}` : `${upstream}-fork.${build}`;

// Line endings may be Windows ones (a Windows checkout), so "\n" in `from` matches either.
function replaceOnce(file, from, to) {
  const full = path.join(root, file);
  const text = fs.readFileSync(full, 'utf8');
  const eol = text.includes('\r\n') ? '\r\n' : '\n';
  const [want, put] = [from, to].map((s) => s.split('\n').join(eol));
  if (!text.includes(want)) throw new Error(`${file}: "${from}" not found`);
  fs.writeFileSync(full, text.replace(want, put));
}

replaceOnce('package.json', `"version": "${upstream}"`, `"version": "${version}"`);
replaceOnce('src-tauri/tauri.conf.json', `"version": "${upstream}"`, `"version": "${version}"`);
replaceOnce('src-tauri/Cargo.toml', `version = "${upstream}"`, `version = "${version}"`);
replaceOnce('src-tauri/Cargo.lock', `name = "recomplauncher"\nversion = "${upstream}"`, `name = "recomplauncher"\nversion = "${version}"`);

console.log(version);
